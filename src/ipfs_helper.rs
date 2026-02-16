use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use crate::{logging, runtime_paths};

#[derive(Debug, Clone)]
pub struct IpfsHelperConfig {
    pub gateways: Vec<String>,
    pub routers: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IpfsHelperFetchResult {
    pub status: u16,
    pub body: Vec<u8>,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchResponseBody {
    pub status: u16,
    #[serde(default, rename = "headers")]
    pub _headers: HashMap<String, String>,
    pub body_base64: String,
}

pub struct IpfsHelperBridge {
    child: Child,
    stdin: ChildStdin,
    stdout_rx: Receiver<std::io::Result<String>>,
    next_id: u64,
}

impl IpfsHelperBridge {
    pub fn spawn(config: IpfsHelperConfig) -> Result<Self> {
        let helper_script = runtime_paths::resolve_ipfs_helper_script()?;
        let node_path = runtime_paths::resolve_node_binary()?;
        tracing::info!(
            node = %node_path,
            script = %helper_script.display(),
            "spawning ipfs helper"
        );
        let gateways_json =
            serde_json::to_string(&config.gateways).context("serialize helper gateways")?;
        let routers_json =
            serde_json::to_string(&config.routers).context("serialize helper routers")?;

        let mut child = Command::new(&node_path)
            .arg(&helper_script)
            .env("VIBEFI_IPFS_HELIA_GATEWAYS", gateways_json)
            .env("VIBEFI_IPFS_HELIA_ROUTERS", routers_json)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to spawn ipfs helper via {}", node_path))?;

        if let Some(stderr) = child.stderr.take() {
            logging::forward_child_stderr("ipfs", stderr);
        } else {
            tracing::warn!("ipfs helper stderr unavailable");
        }

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("ipfs helper stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("ipfs helper stdout unavailable"))?;
        let stdout_rx = spawn_stdout_reader(stdout);
        let mut bridge = Self {
            child,
            stdin,
            stdout_rx,
            next_id: 1,
        };

        bridge
            .ping()
            .context("ipfs helper failed ping; run `cd client/ipfs-helper && bun install` first")?;
        Ok(bridge)
    }

    pub fn fetch(&mut self, url: &str, timeout_ms: Option<u64>) -> Result<IpfsHelperFetchResult> {
        let mut payload = serde_json::json!({ "url": url });
        if let Some(timeout_ms) = timeout_ms {
            payload["timeoutMs"] = Value::from(timeout_ms);
        }
        let helper_timeout = timeout_ms
            .and_then(|ms| ms.checked_add(10_000))
            .unwrap_or(40_000);
        let result = self.send_command("fetch", payload, Duration::from_millis(helper_timeout))?;
        let parsed: FetchResponseBody =
            serde_json::from_value(result).context("invalid fetch response from helper")?;
        let body = base64::engine::general_purpose::STANDARD
            .decode(parsed.body_base64)
            .context("decode helper bodyBase64")?;
        Ok(IpfsHelperFetchResult {
            status: parsed.status,
            body,
        })
    }

    fn ping(&mut self) -> Result<()> {
        let _ = self.send_command("ping", Value::Null, Duration::from_secs(10))?;
        Ok(())
    }

    fn send_command(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let payload = serde_json::json!({
            "id": id,
            "method": method,
            "params": params
        });
        let line = serde_json::to_string(&payload)?;
        tracing::debug!(method, id, "ipfs helper send");
        self.stdin
            .write_all(line.as_bytes())
            .context("failed writing helper request")?;
        self.stdin
            .write_all(b"\n")
            .context("failed writing helper newline")?;
        self.stdin
            .flush()
            .context("failed flushing helper request")?;
        tracing::debug!(method, id, "ipfs helper request flushed");

        let deadline = Instant::now() + timeout;
        loop {
            let now = Instant::now();
            if now >= deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                bail!(
                    "ipfs helper timed out waiting for {} response after {}ms",
                    method,
                    timeout.as_millis()
                );
            }
            let wait_for = deadline.saturating_duration_since(now);
            let raw = match self.stdout_rx.recv_timeout(wait_for) {
                Ok(line) => line.context("failed reading helper response")?,
                Err(RecvTimeoutError::Timeout) => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    bail!(
                        "ipfs helper timed out waiting for {} response after {}ms",
                        method,
                        timeout.as_millis()
                    );
                }
                Err(RecvTimeoutError::Disconnected) => {
                    if let Ok(Some(status)) = self.child.try_wait() {
                        bail!("ipfs helper exited unexpectedly: {}", status);
                    }
                    bail!("ipfs helper closed pipe unexpectedly");
                }
            };
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            let response: HelperResponse =
                serde_json::from_str(raw).context("invalid helper response payload")?;
            tracing::debug!(
                id = response.id,
                ok = response.result.is_some(),
                has_error = response.error.is_some(),
                "ipfs helper recv"
            );
            if response.id != id {
                bail!(
                    "ipfs helper returned mismatched id (expected {}, got {})",
                    id,
                    response.id
                );
            }
            if let Some(error) = response.error {
                bail!("ipfs helper error {}: {}", error.code, error.message);
            }
            return Ok(response.result.unwrap_or(Value::Null));
        }
    }
}

fn spawn_stdout_reader(stdout: ChildStdout) -> Receiver<std::io::Result<String>> {
    let (tx, rx) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("ipfs-helper-stdout".to_string())
        .spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    rx
}

impl Drop for IpfsHelperBridge {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
