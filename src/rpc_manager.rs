use anyhow::{Result, anyhow, bail};
use reqwest::blocking::Client as HttpClient;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcEndpoint {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

struct EndpointHealth {
    endpoint: RpcEndpoint,
    consecutive_failures: u32,
    backoff_until: Option<Instant>,
}

pub struct RpcEndpointManager {
    endpoints: Vec<EndpointHealth>,
    http: HttpClient,
    active_index: usize,
}

impl RpcEndpointManager {
    pub fn new(endpoints: Vec<RpcEndpoint>, http: HttpClient) -> Self {
        let health: Vec<EndpointHealth> = endpoints
            .into_iter()
            .map(|ep| EndpointHealth {
                endpoint: ep,
                consecutive_failures: 0,
                backoff_until: None,
            })
            .collect();
        tracing::info!(endpoints = health.len(), "rpc endpoint manager initialized");
        Self {
            endpoints: health,
            http,
            active_index: 0,
        }
    }

    pub fn send_rpc(&mut self, payload: &Value) -> Result<Value> {
        if self.endpoints.is_empty() {
            tracing::error!("rpc endpoint manager has no configured endpoints");
            bail!("No RPC endpoints configured");
        }

        let max_retries = 3usize;
        let mut last_error: Option<anyhow::Error> = None;
        let method = payload
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("unknown");

        tracing::debug!(
            method,
            endpoints = self.endpoints.len(),
            retries = max_retries,
            "rpc send start"
        );

        for attempt in 0..max_retries {
            let idx = self.pick_endpoint();
            let endpoint = &self.endpoints[idx].endpoint;
            let url = endpoint.url.clone();
            let label = endpoint.label.as_deref().unwrap_or("");
            tracing::debug!(
                method,
                attempt = attempt + 1,
                endpoint_index = idx,
                endpoint_url = %url,
                endpoint_label = %label,
                "rpc attempt"
            );

            match self.try_send(&url, payload) {
                Ok(body) => {
                    // Check for JSON-RPC level error
                    if body.get("error").is_some() {
                        // Non-transient JSON-RPC error — return immediately
                        let rpc_error = body
                            .get("error")
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "null".to_string());
                        tracing::warn!(
                            method,
                            endpoint_index = idx,
                            endpoint_url = %url,
                            error = %rpc_error,
                            "rpc json-rpc error response"
                        );
                        return Ok(body);
                    }
                    // Success: reset failure count
                    let previous_failures = self.endpoints[idx].consecutive_failures;
                    self.endpoints[idx].consecutive_failures = 0;
                    self.endpoints[idx].backoff_until = None;
                    tracing::debug!(
                        method,
                        endpoint_index = idx,
                        endpoint_url = %url,
                        previous_failures,
                        "rpc success"
                    );
                    return Ok(body);
                }
                Err(e) => {
                    // Mark transient failure
                    let health = &mut self.endpoints[idx];
                    health.consecutive_failures += 1;
                    let n = health.consecutive_failures;
                    let backoff_ms = (500u64 * (1u64 << (n - 1).min(4))).min(10_000);
                    health.backoff_until =
                        Some(Instant::now() + std::time::Duration::from_millis(backoff_ms));
                    self.advance_active();
                    tracing::warn!(
                        method,
                        endpoint_index = idx,
                        endpoint_url = %url,
                        consecutive_failures = n,
                        backoff_ms,
                        error = %e,
                        "rpc endpoint attempt failed"
                    );
                    last_error = Some(e);
                }
            }
        }

        tracing::error!(
            method,
            retries = max_retries,
            last_error = %last_error
                .as_ref()
                .map(|e| e.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            "all rpc endpoints failed"
        );
        Err(last_error.unwrap_or_else(|| anyhow!("All RPC endpoints failed")))
    }

    pub fn get_endpoints(&self) -> Vec<RpcEndpoint> {
        self.endpoints.iter().map(|h| h.endpoint.clone()).collect()
    }

    pub fn set_endpoints(&mut self, endpoints: Vec<RpcEndpoint>) {
        self.endpoints = endpoints
            .into_iter()
            .map(|ep| EndpointHealth {
                endpoint: ep,
                consecutive_failures: 0,
                backoff_until: None,
            })
            .collect();
        self.active_index = 0;
        tracing::info!(endpoints = self.endpoints.len(), "rpc endpoints updated");
    }

    fn pick_endpoint(&self) -> usize {
        let now = Instant::now();

        // Try active index first if not in backoff
        if self.active_index < self.endpoints.len() {
            let health = &self.endpoints[self.active_index];
            if health.backoff_until.map_or(true, |t| now >= t) {
                return self.active_index;
            }
        }

        // Find first endpoint not in backoff
        for (i, h) in self.endpoints.iter().enumerate() {
            if h.backoff_until.map_or(true, |t| now >= t) {
                return i;
            }
        }

        // All in backoff — return active index; send_rpc will retry
        // and the backoff window will naturally pass on the next call.
        self.active_index
    }

    fn advance_active(&mut self) {
        if self.endpoints.len() > 1 {
            let previous = self.active_index;
            self.active_index = (self.active_index + 1) % self.endpoints.len();
            tracing::debug!(
                from = previous,
                to = self.active_index,
                "advanced rpc active endpoint"
            );
        }
    }

    fn try_send(&self, url: &str, payload: &Value) -> Result<Value> {
        let res = self.http.post(url).json(payload).send();
        match res {
            Ok(response) => {
                let status = response.status();
                if status == 429 || status.is_server_error() {
                    bail!("HTTP {}: transient error from {}", status, url);
                }
                if status.is_client_error() {
                    bail!("HTTP {}: client error from {}", status, url);
                }
                let body: Value = response
                    .json()
                    .map_err(|e| anyhow!("Failed to decode RPC response from {}: {}", url, e))?;
                Ok(body)
            }
            Err(e) => {
                bail!("Connection error to {}: {}", url, e);
            }
        }
    }
}
