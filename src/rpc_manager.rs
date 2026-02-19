use anyhow::{Result, anyhow, bail};
use reqwest::blocking::Client as HttpClient;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

pub const DEFAULT_MAX_CONCURRENT_RPC: usize = 10;

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

struct HealthState {
    endpoints: Vec<EndpointHealth>,
    active_index: usize,
}

struct SemaphoreState {
    max: usize,
    in_flight: usize,
}

struct Semaphore {
    state: Mutex<SemaphoreState>,
    condvar: Condvar,
}

impl Semaphore {
    fn new(max: usize) -> Self {
        Self {
            state: Mutex::new(SemaphoreState { max, in_flight: 0 }),
            condvar: Condvar::new(),
        }
    }

    fn acquire(&self) {
        let mut s = self.state.lock().expect("semaphore lock");
        while s.in_flight >= s.max {
            s = self.condvar.wait(s).expect("semaphore wait");
        }
        s.in_flight += 1;
    }

    fn release(&self) {
        let mut s = self.state.lock().expect("semaphore lock");
        s.in_flight = s.in_flight.saturating_sub(1);
        drop(s);
        self.condvar.notify_one();
    }

    fn set_max(&self, max: usize) {
        let mut s = self.state.lock().expect("semaphore lock");
        s.max = max;
        drop(s);
        self.condvar.notify_all();
    }

    fn get_max(&self) -> usize {
        self.state.lock().expect("semaphore lock").max
    }
}

/// RAII guard that releases the semaphore on drop, ensuring release always fires
/// even if `send_rpc` returns early via `?` or explicit `return`.
struct SemaphoreGuard(Arc<Semaphore>);

impl Drop for SemaphoreGuard {
    fn drop(&mut self) {
        self.0.release();
    }
}

#[derive(Clone)]
pub struct RpcEndpointManager {
    health: Arc<Mutex<HealthState>>,
    semaphore: Arc<Semaphore>,
    http: HttpClient,
}

impl RpcEndpointManager {
    pub fn new(endpoints: Vec<RpcEndpoint>, http: HttpClient, max_concurrent: usize) -> Self {
        let endpoints: Vec<EndpointHealth> = endpoints
            .into_iter()
            .map(|ep| EndpointHealth {
                endpoint: ep,
                consecutive_failures: 0,
                backoff_until: None,
            })
            .collect();
        tracing::info!(
            endpoints = endpoints.len(),
            max_concurrent,
            "rpc endpoint manager initialized"
        );
        Self {
            health: Arc::new(Mutex::new(HealthState {
                endpoints,
                active_index: 0,
            })),
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            http,
        }
    }

    pub fn send_rpc(&self, payload: &Value) -> Result<Value> {
        let method = payload
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("unknown");

        let n_endpoints = {
            let h = self.health.lock().expect("rpc health lock");
            if h.endpoints.is_empty() {
                bail!("No RPC endpoints configured");
            }
            h.endpoints.len()
        };

        let max_retries = 3usize;
        let mut last_error: Option<anyhow::Error> = None;

        tracing::debug!(method, endpoints = n_endpoints, retries = max_retries, "rpc send start");

        // Acquire a concurrency slot. Released automatically when _guard is dropped.
        self.semaphore.acquire();
        let _guard = SemaphoreGuard(Arc::clone(&self.semaphore));

        for attempt in 0..max_retries {
            // Lock briefly to pick an endpoint — released before the HTTP call.
            let (idx, url, label) = {
                let h = self.health.lock().expect("rpc health lock");
                let idx = Self::pick_endpoint_idx(&h);
                let ep = &h.endpoints[idx].endpoint;
                (idx, ep.url.clone(), ep.label.as_deref().unwrap_or("").to_string())
            };

            tracing::debug!(
                method,
                attempt = attempt + 1,
                endpoint_index = idx,
                endpoint_url = %url,
                endpoint_label = %label,
                "rpc attempt"
            );

            // HTTP call — no locks held.
            match self.try_send(&url, payload) {
                Ok(body) => {
                    if body.get("error").is_some() {
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
                    // Lock briefly to record success.
                    let previous_failures = {
                        let mut h = self.health.lock().expect("rpc health lock");
                        let pf = h.endpoints[idx].consecutive_failures;
                        h.endpoints[idx].consecutive_failures = 0;
                        h.endpoints[idx].backoff_until = None;
                        pf
                    };
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
                    // Lock briefly to record failure and advance endpoint.
                    let (n, backoff_ms) = {
                        let mut h = self.health.lock().expect("rpc health lock");
                        let health = &mut h.endpoints[idx];
                        health.consecutive_failures += 1;
                        let n = health.consecutive_failures;
                        let backoff_ms = (500u64 * (1u64 << (n - 1).min(4))).min(10_000);
                        health.backoff_until =
                            Some(Instant::now() + std::time::Duration::from_millis(backoff_ms));
                        Self::advance_active_idx(&mut h);
                        (n, backoff_ms)
                    };
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
        self.health
            .lock()
            .expect("rpc health lock")
            .endpoints
            .iter()
            .map(|h| h.endpoint.clone())
            .collect()
    }

    pub fn set_endpoints(&self, endpoints: Vec<RpcEndpoint>) {
        let mut h = self.health.lock().expect("rpc health lock");
        h.endpoints = endpoints
            .into_iter()
            .map(|ep| EndpointHealth {
                endpoint: ep,
                consecutive_failures: 0,
                backoff_until: None,
            })
            .collect();
        h.active_index = 0;
        tracing::info!(endpoints = h.endpoints.len(), "rpc endpoints updated");
    }

    pub fn get_max_concurrent(&self) -> usize {
        self.semaphore.get_max()
    }

    pub fn set_max_concurrent(&self, max: usize) {
        let max = max.max(1);
        self.semaphore.set_max(max);
        tracing::info!(max, "rpc max concurrent updated");
    }

    fn pick_endpoint_idx(h: &HealthState) -> usize {
        let now = Instant::now();
        if h.active_index < h.endpoints.len()
            && h.endpoints[h.active_index]
                .backoff_until
                .map_or(true, |t| now >= t)
        {
            return h.active_index;
        }
        for (i, health) in h.endpoints.iter().enumerate() {
            if health.backoff_until.map_or(true, |t| now >= t) {
                return i;
            }
        }
        h.active_index
    }

    fn advance_active_idx(h: &mut HealthState) {
        if h.endpoints.len() > 1 {
            let previous = h.active_index;
            h.active_index = (h.active_index + 1) % h.endpoints.len();
            tracing::debug!(
                from = previous,
                to = h.active_index,
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
