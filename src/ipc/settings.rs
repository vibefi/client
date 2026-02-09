use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::ipc_contract::IpcRequest;
use crate::rpc_manager::RpcEndpoint;
use crate::state::AppState;

pub(super) fn handle_settings_ipc(state: &AppState, req: &IpcRequest) -> Result<Value> {
    match req.method.as_str() {
        "vibefi_getEndpoints" => {
            let mgr = state.rpc_manager.lock().unwrap();
            let endpoints = match mgr.as_ref() {
                Some(m) => m.get_endpoints(),
                None => Vec::new(),
            };
            Ok(serde_json::to_value(endpoints)?)
        }
        "vibefi_setEndpoints" => {
            let endpoints: Vec<RpcEndpoint> = serde_json::from_value(
                req.params
                    .get(0)
                    .cloned()
                    .ok_or_else(|| anyhow!("missing endpoints parameter"))?,
            )?;

            // Update the live manager
            {
                let mut mgr = state.rpc_manager.lock().unwrap();
                if let Some(m) = mgr.as_mut() {
                    m.set_endpoints(endpoints.clone());
                }
            }

            // Persist to disk
            if let Some(ref config_path) = state.config_path {
                let settings = crate::settings::UserSettings {
                    rpc_endpoints: endpoints,
                };
                crate::settings::save_settings(config_path, &settings)?;
            }

            Ok(Value::Bool(true))
        }
        _ => Err(anyhow!("Unsupported settings method: {}", req.method)),
    }
}
