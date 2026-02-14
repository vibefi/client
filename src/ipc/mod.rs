mod hardware;
mod local;
mod router;
mod rpc;
mod selector;
mod settings;
mod walletconnect;

use anyhow::Result;
use serde_json::Value;
use wry::WebView;

use crate::ipc_contract::IpcRequest;
use crate::state::{AppState, UserEvent};

pub use router::handle_ipc;
pub use walletconnect::handle_walletconnect_connect_result;

pub fn respond_ok(webview: &WebView, id: u64, value: Value) -> Result<()> {
    crate::ui_bridge::respond_ok(webview, id, value)
}

pub fn respond_err(webview: &WebView, id: u64, message: &str) -> Result<()> {
    crate::ui_bridge::respond_err(webview, id, message)
}

pub fn respond_option_result(
    webview: &WebView,
    id: u64,
    result: Result<Option<Value>>,
) -> Result<()> {
    match result {
        Ok(Some(value)) => respond_ok(webview, id, value),
        Ok(None) => Ok(()), // Deferred response.
        Err(err) => respond_err(webview, id, &err.to_string()),
    }
}

pub fn respond_value_result(
    webview: &WebView,
    id: u64,
    result: std::result::Result<Value, String>,
) -> Result<()> {
    match result {
        Ok(value) => respond_ok(webview, id, value),
        Err(message) => respond_err(webview, id, &message),
    }
}

pub fn network_identity_response(state: &AppState, method: &str) -> Option<Value> {
    match method {
        "eth_chainId" => Some(Value::String(state.chain_id_hex())),
        "net_version" => {
            let chain_id = state
                .wallet
                .lock()
                .expect("poisoned wallet lock while handling net_version")
                .chain
                .chain_id;
            Some(Value::String(chain_id.to_string()))
        }
        _ => None,
    }
}

pub fn try_spawn_rpc_passthrough(state: &AppState, webview_id: &str, req: &IpcRequest) -> bool {
    if state.network.is_none() || !rpc::is_rpc_passthrough(req.method.as_str()) {
        return false;
    }

    let proxy = state.proxy.clone();
    let state_clone = state.clone();
    let ipc_id = req.id;
    let method = req.method.clone();
    let params = req.params.clone();
    let wv_id = webview_id.to_string();
    tracing::debug!(
        webview_id,
        ipc_id = ipc_id,
        method = %method,
        "spawning rpc passthrough worker"
    );
    std::thread::spawn(move || {
        let request = IpcRequest {
            id: ipc_id,
            provider_id: None,
            method,
            params,
        };
        let result = rpc::proxy_rpc(&state_clone, &request).map_err(|e| e.to_string());
        if let Err(err) = &result {
            tracing::warn!(
                webview_id = %wv_id,
                ipc_id,
                method = %request.method,
                error = %err,
                "rpc passthrough worker failed"
            );
        } else {
            tracing::debug!(
                webview_id = %wv_id,
                ipc_id,
                method = %request.method,
                "rpc passthrough worker succeeded"
            );
        }
        if let Err(err) = proxy.send_event(UserEvent::RpcResult {
            webview_id: wv_id,
            ipc_id,
            result,
        }) {
            tracing::warn!(error = %err, "failed to send RpcResult event from passthrough worker");
        }
    });

    true
}

pub fn emit_accounts_changed(webview: &WebView, addrs: Vec<String>) {
    crate::ui_bridge::emit_accounts_changed(webview, addrs);
}

pub fn emit_chain_changed(webview: &WebView, chain_id_hex: String) {
    crate::ui_bridge::emit_chain_changed(webview, chain_id_hex);
}
