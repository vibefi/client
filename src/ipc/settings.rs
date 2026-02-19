use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::Path;

use crate::config::IpfsFetchBackend;
use crate::ipc_contract::IpcRequest;
use crate::rpc_manager::{DEFAULT_MAX_CONCURRENT_RPC, RpcEndpoint};
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IpfsSettingsResponse {
    fetch_backend: IpfsFetchBackend,
    gateway_endpoint: String,
    default_gateway_endpoint: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetIpfsSettingsRequest {
    fetch_backend: IpfsFetchBackend,
    #[serde(default)]
    gateway_endpoint: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetRpcAndIpfsSettingsRequest {
    max_concurrent_rpc: usize,
    fetch_backend: IpfsFetchBackend,
    #[serde(default)]
    gateway_endpoint: Option<String>,
}

fn open_directory_in_file_manager(path: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("open")
            .arg(path)
            .status()
            .with_context(|| format!("failed to run 'open' for {}", path.display()))?;
        if !status.success() {
            bail!("'open' exited with status {}", status);
        }
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let status = std::process::Command::new("explorer")
            .arg(path)
            .status()
            .with_context(|| format!("failed to run 'explorer' for {}", path.display()))?;
        if !status.success() {
            bail!("'explorer' exited with status {}", status);
        }
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let status = std::process::Command::new("xdg-open")
            .arg(path)
            .status()
            .with_context(|| format!("failed to run 'xdg-open' for {}", path.display()))?;
        if !status.success() {
            bail!("'xdg-open' exited with status {}", status);
        }
        return Ok(());
    }

    #[allow(unreachable_code)]
    Ok(())
}

pub(super) fn handle_settings_ipc(state: &AppState, req: &IpcRequest) -> Result<Value> {
    match req.method.as_str() {
        "vibefi_getEndpoints" => {
            let mgr = state
                .rpc_manager
                .lock()
                .expect("poisoned rpc_manager lock while reading settings endpoints");
            let endpoints = match mgr.as_ref() {
                Some(m) => m.get_endpoints(),
                None => Vec::new(),
            };
            tracing::debug!(count = endpoints.len(), "settings get rpc endpoints");
            Ok(serde_json::to_value(endpoints)?)
        }
        "vibefi_setEndpoints" => {
            let endpoints: Vec<RpcEndpoint> = serde_json::from_value(
                req.params
                    .get(0)
                    .cloned()
                    .ok_or_else(|| anyhow!("missing endpoints parameter"))?,
            )?;
            if endpoints.is_empty() {
                return Err(anyhow!("At least one RPC endpoint is required"));
            }
            if endpoints.iter().any(|ep| ep.url.trim().is_empty()) {
                return Err(anyhow!("RPC endpoint URL cannot be empty"));
            }
            tracing::info!(count = endpoints.len(), "settings set rpc endpoints");

            // Update the live manager
            {
                let mgr = state
                    .rpc_manager
                    .lock()
                    .expect("poisoned rpc_manager lock while updating settings endpoints");
                if let Some(m) = mgr.as_ref() {
                    m.set_endpoints(endpoints.clone());
                }
            }

            // Persist to disk
            if let Some(ref config_path) =
                state.resolved.as_ref().and_then(|r| r.config_path.clone())
            {
                let mut settings = crate::settings::load_settings(config_path);
                settings.rpc_endpoints = endpoints;
                crate::settings::save_settings(config_path, &settings)?;
            }

            Ok(Value::Bool(true))
        }
        "vibefi_getIpfsSettings" => {
            let default_backend = state
                .resolved
                .as_ref()
                .map(|r| r.ipfs_fetch_backend)
                .unwrap_or_default();
            let default_gateway_endpoint = state
                .resolved
                .as_ref()
                .map(|r| r.ipfs_gateway.clone())
                .unwrap_or_else(|| "http://127.0.0.1:8080".to_string());

            let user_settings = state
                .resolved
                .as_ref()
                .and_then(|r| r.config_path.as_ref())
                .map(|p| crate::settings::load_settings(p))
                .unwrap_or_default();
            let fetch_backend = user_settings.ipfs.fetch_backend.unwrap_or(default_backend);
            let gateway_endpoint = user_settings
                .ipfs
                .gateway_endpoint
                .unwrap_or_else(|| default_gateway_endpoint.clone());
            tracing::debug!(
                backend = fetch_backend.as_str(),
                "settings get ipfs settings"
            );

            Ok(serde_json::to_value(IpfsSettingsResponse {
                fetch_backend,
                gateway_endpoint,
                default_gateway_endpoint,
            })?)
        }
        "vibefi_setIpfsSettings" => {
            let params: SetIpfsSettingsRequest = serde_json::from_value(
                req.params
                    .get(0)
                    .cloned()
                    .ok_or_else(|| anyhow!("missing ipfs settings parameter"))?,
            )?;
            tracing::info!(
                backend = params.fetch_backend.as_str(),
                "settings set ipfs settings"
            );

            if let Some(ref config_path) =
                state.resolved.as_ref().and_then(|r| r.config_path.clone())
            {
                let mut settings = crate::settings::load_settings(config_path);
                settings.ipfs.fetch_backend = Some(params.fetch_backend);
                settings.ipfs.gateway_endpoint = params
                    .gateway_endpoint
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned);
                crate::settings::save_settings(config_path, &settings)?;
            }

            Ok(Value::Bool(true))
        }
        "vibefi_saveSettings" | "vibefi_setRpcAndIpfsSettings" => {
            let params: SetRpcAndIpfsSettingsRequest = serde_json::from_value(
                req.params
                    .get(0)
                    .cloned()
                    .ok_or_else(|| anyhow!("missing settings parameter"))?,
            )?;
            if params.max_concurrent_rpc < 1 {
                return Err(anyhow!("maxConcurrentRpc must be a whole number >= 1"));
            }
            tracing::info!(
                max_concurrent_rpc = params.max_concurrent_rpc,
                backend = params.fetch_backend.as_str(),
                "settings set rpc and ipfs settings"
            );

            {
                let mgr = state
                    .rpc_manager
                    .lock()
                    .expect("poisoned rpc_manager lock while updating max concurrent rpc");
                if let Some(m) = mgr.as_ref() {
                    m.set_max_concurrent(params.max_concurrent_rpc);
                }
            }

            if let Some(ref config_path) =
                state.resolved.as_ref().and_then(|r| r.config_path.clone())
            {
                let mut settings = crate::settings::load_settings(config_path);
                settings.max_concurrent_rpc = Some(params.max_concurrent_rpc);
                settings.ipfs.fetch_backend = Some(params.fetch_backend);
                settings.ipfs.gateway_endpoint = params
                    .gateway_endpoint
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned);
                crate::settings::save_settings(config_path, &settings)?;
            }

            Ok(Value::Bool(true))
        }
        "vibefi_getMaxConcurrentRpc" => {
            let mgr = state
                .rpc_manager
                .lock()
                .expect("poisoned rpc_manager lock while reading max concurrent rpc");
            let max = mgr
                .as_ref()
                .map(|m| m.get_max_concurrent())
                .unwrap_or(DEFAULT_MAX_CONCURRENT_RPC);
            Ok(Value::Number(max.into()))
        }
        "vibefi_setMaxConcurrentRpc" => {
            let max: usize = serde_json::from_value(
                req.params
                    .get(0)
                    .cloned()
                    .ok_or_else(|| anyhow!("missing max parameter"))?,
            )?;
            {
                let mgr = state
                    .rpc_manager
                    .lock()
                    .expect("poisoned rpc_manager lock while updating max concurrent rpc");
                if let Some(m) = mgr.as_ref() {
                    m.set_max_concurrent(max);
                }
            }
            if let Some(ref config_path) =
                state.resolved.as_ref().and_then(|r| r.config_path.clone())
            {
                let mut settings = crate::settings::load_settings(config_path);
                settings.max_concurrent_rpc = Some(max);
                crate::settings::save_settings(config_path, &settings)?;
            }
            Ok(Value::Bool(true))
        }
        "vibefi_openLogDirectory" => {
            let log_dir = crate::runtime_paths::resolve_log_dir();
            fs::create_dir_all(&log_dir)
                .with_context(|| format!("failed to create log dir {}", log_dir.display()))?;
            open_directory_in_file_manager(&log_dir)?;
            Ok(Value::String(log_dir.to_string_lossy().into_owned()))
        }
        _ => Err(anyhow!("Unsupported settings method: {}", req.method)),
    }
}
