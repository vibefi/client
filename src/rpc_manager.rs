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
        Self {
            endpoints: health,
            http,
            active_index: 0,
        }
    }

    pub fn send_rpc(&mut self, payload: &Value) -> Result<Value> {
        if self.endpoints.is_empty() {
            bail!("No RPC endpoints configured");
        }

        let max_retries = 3usize;
        let mut last_error: Option<anyhow::Error> = None;

        for _ in 0..max_retries {
            let idx = self.pick_endpoint();
            let url = self.endpoints[idx].endpoint.url.clone();

            match self.try_send(&url, payload) {
                Ok(body) => {
                    // Check for JSON-RPC level error
                    if body.get("error").is_some() {
                        // Non-transient JSON-RPC error — return immediately
                        return Ok(body);
                    }
                    // Success: reset failure count
                    self.endpoints[idx].consecutive_failures = 0;
                    self.endpoints[idx].backoff_until = None;
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
                    last_error = Some(e);
                }
            }
        }

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

        // All in backoff — find earliest backoff expiry and sleep
        if let Some(earliest) = self.endpoints.iter().filter_map(|h| h.backoff_until).min() {
            if earliest > now {
                std::thread::sleep(earliest - now);
            }
        }

        // Return active index after sleeping
        self.active_index
    }

    fn advance_active(&mut self) {
        if self.endpoints.len() > 1 {
            self.active_index = (self.active_index + 1) % self.endpoints.len();
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
