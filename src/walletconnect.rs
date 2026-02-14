use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use crate::{logging, runtime_paths};

#[derive(Debug, Clone)]
pub struct WalletConnectConfig {
    pub project_id: String,
    pub relay_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WalletConnectSession {
    pub accounts: Vec<String>,
    pub chain_id_hex: String,
}

#[derive(Debug, Deserialize)]
struct HelperResponse {
    pub id: u64,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<HelperError>,
}

#[derive(Debug, Deserialize)]
struct HelperError {
    pub code: i64,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HelperEvent {
    pub event: String,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    #[serde(rename = "qrSvg")]
    pub qr_svg: Option<String>,
    #[serde(default)]
    pub accounts: Option<Vec<String>>,
    #[serde(default)]
    #[serde(rename = "chainId")]
    pub chain_id: Option<String>,
}

enum BridgeMessage {
    Response(HelperResponse),
    Event(HelperEvent),
}

pub struct WalletConnectBridge {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl WalletConnectBridge {
    pub fn spawn(config: WalletConnectConfig) -> Result<Self> {
        if config.project_id.trim().is_empty() {
            bail!("WalletConnect project id missing");
        }

        let helper_script = runtime_paths::resolve_wc_helper_script()?;
        let node_path = runtime_paths::resolve_node_binary()?;
        let mut child = Command::new(&node_path)
            .arg(&helper_script)
            .env("VIBEFI_WC_PROJECT_ID", config.project_id)
            .env("VIBEFI_WC_RELAY_URL", config.relay_url.unwrap_or_default())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to spawn walletconnect helper via {}", node_path))?;

        if let Some(stderr) = child.stderr.take() {
            logging::forward_child_stderr("walletconnect", stderr);
        } else {
            tracing::warn!("walletconnect helper stderr unavailable");
        }

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("walletconnect helper stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("walletconnect helper stdout unavailable"))?;
        let mut bridge = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        };

        bridge.ping().context(
            "walletconnect helper failed ping; run `cd client/walletconnect-helper && bun install` first",
        )?;
        Ok(bridge)
    }

    pub fn connect_with_event_handler<F>(
        &mut self,
        chain_id: u64,
        mut on_event: F,
    ) -> Result<WalletConnectSession>
    where
        F: FnMut(&HelperEvent),
    {
        tracing::info!(
            chain_id = format!("0x{:x}", chain_id),
            "walletconnect connect requested; waiting for wallet approval"
        );
        let result = self.send_command_with_event_handler(
            "connect",
            serde_json::json!({
                "chainId": format!("0x{:x}", chain_id)
            }),
            |event| on_event(event),
        )?;
        let response: ConnectResponse =
            serde_json::from_value(result).context("invalid connect response from helper")?;
        Ok(WalletConnectSession {
            accounts: response.accounts,
            chain_id_hex: response.chain_id,
        })
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<(Value, Vec<HelperEvent>)> {
        self.send_command(
            "request",
            serde_json::json!({
                "method": method,
                "params": params
            }),
        )
    }

    pub fn disconnect(&mut self) -> Result<()> {
        let _ = self.send_command("disconnect", Value::Null)?;
        Ok(())
    }

    fn ping(&mut self) -> Result<()> {
        let _ = self.send_command("ping", Value::Null)?;
        Ok(())
    }

    fn send_command(&mut self, method: &str, params: Value) -> Result<(Value, Vec<HelperEvent>)> {
        let mut events = Vec::new();
        let result = self.send_command_with_event_handler(method, params, |event| {
            events.push(event.clone());
        })?;
        Ok((result, events))
    }

    fn send_command_with_event_handler<F>(
        &mut self,
        method: &str,
        params: Value,
        mut on_event: F,
    ) -> Result<Value>
    where
        F: FnMut(&HelperEvent),
    {
        let id = self.next_id;
        self.next_id += 1;
        let payload = serde_json::json!({
            "id": id,
            "method": method,
            "params": params
        });
        let line = serde_json::to_string(&payload)?;
        self.stdin
            .write_all(line.as_bytes())
            .context("failed writing helper request")?;
        self.stdin
            .write_all(b"\n")
            .context("failed writing helper newline")?;
        self.stdin
            .flush()
            .context("failed flushing helper request")?;

        loop {
            let mut raw = String::new();
            let n = self
                .stdout
                .read_line(&mut raw)
                .context("failed reading helper response")?;
            if n == 0 {
                bail!("walletconnect helper closed pipe unexpectedly");
            }
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            match parse_bridge_line(raw)? {
                BridgeMessage::Event(event) => {
                    log_helper_event(&event);
                    on_event(&event);
                    continue;
                }
                BridgeMessage::Response(resp) => {
                    if resp.id != id {
                        bail!(
                            "walletconnect helper returned mismatched id (expected {}, got {})",
                            id,
                            resp.id
                        );
                    }
                    if let Some(error) = resp.error {
                        bail!(
                            "walletconnect helper error {}: {}",
                            error.code,
                            error.message
                        );
                    }
                    return Ok(resp.result.unwrap_or(Value::Null));
                }
            }
        }
    }
}

impl Drop for WalletConnectBridge {
    fn drop(&mut self) {
        let _ = self.disconnect();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Deserialize)]
struct ConnectResponse {
    pub accounts: Vec<String>,
    #[serde(rename = "chainId")]
    pub chain_id: String,
}

fn parse_bridge_line(raw: &str) -> Result<BridgeMessage> {
    let value: Value = serde_json::from_str(raw).context("helper output is not valid json")?;
    if value.get("event").is_some() {
        let event: HelperEvent = serde_json::from_value(value).context("invalid helper event")?;
        return Ok(BridgeMessage::Event(event));
    }
    let response: HelperResponse =
        serde_json::from_value(value).context("invalid helper response payload")?;
    Ok(BridgeMessage::Response(response))
}

fn log_helper_event(event: &HelperEvent) {
    match event.event.as_str() {
        "display_uri" => {
            if let Some(uri) = event.uri.as_deref() {
                tracing::info!(pairing_uri = %redact_uri(uri), "walletconnect pairing uri");
            } else {
                tracing::info!("walletconnect pairing uri event received");
            }
        }
        "accountsChanged" => {
            let count = event.accounts.as_ref().map(|a| a.len()).unwrap_or(0);
            tracing::info!(count, "walletconnect accountsChanged");
        }
        "chainChanged" => {
            let chain = event.chain_id.as_deref().unwrap_or("unknown");
            tracing::info!(chain, "walletconnect chainChanged");
        }
        "disconnect" => {
            tracing::info!("walletconnect disconnect");
        }
        _ => {
            tracing::debug!(event = %event.event, "walletconnect event");
        }
    }
}

fn redact_uri(uri: &str) -> String {
    const PREFIX_LEN: usize = 18;
    const SUFFIX_LEN: usize = 6;
    if uri.len() <= PREFIX_LEN + SUFFIX_LEN {
        return "<redacted>".to_string();
    }
    format!(
        "{}...{}",
        &uri[..PREFIX_LEN],
        &uri[uri.len().saturating_sub(SUFFIX_LEN)..]
    )
}
