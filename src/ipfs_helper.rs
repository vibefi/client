use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
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
    stdout: BufReader<ChildStdout>,
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
        let mut bridge = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
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
        let result = self.send_command("fetch", payload)?;
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
        let _ = self.send_command("ping", Value::Null)?;
        Ok(())
    }

    fn send_command(&mut self, method: &str, params: Value) -> Result<Value> {
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

        loop {
            let mut raw = String::new();
            let n = self
                .stdout
                .read_line(&mut raw)
                .context("failed reading helper response")?;
            if n == 0 {
                bail!("ipfs helper closed pipe unexpectedly");
            }
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

impl Drop for IpfsHelperBridge {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
