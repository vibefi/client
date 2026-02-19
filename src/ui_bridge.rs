use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use wry::WebView;

use crate::ipc_contract::{
    HostDispatchEnvelope, HostDispatchKind, ProviderEventPayload, RpcResponseError,
    RpcResponsePayload, RpcStatusPayload, TabbarUpdatePayload, WalletconnectPairingPayload,
};

fn dispatch<T: Serialize>(webview: &WebView, kind: HostDispatchKind, payload: T) -> Result<()> {
    let envelope = HostDispatchEnvelope { kind, payload };
    let script = format!(
        "window.__VibefiHostDispatch({});",
        serde_json::to_string(&envelope)?
    );
    webview.evaluate_script(&script)?;
    Ok(())
}

pub fn respond_ok(webview: &WebView, id: u64, value: Value) -> Result<()> {
    dispatch(
        webview,
        HostDispatchKind::RpcResponse,
        RpcResponsePayload {
            id,
            result: value,
            error: None,
        },
    )
}

pub fn respond_err(webview: &WebView, id: u64, message: &str) -> Result<()> {
    dispatch(
        webview,
        HostDispatchKind::RpcResponse,
        RpcResponsePayload {
            id,
            result: Value::Null,
            error: Some(RpcResponseError {
                code: -32601,
                message: message.to_string(),
            }),
        },
    )
}

pub fn emit_accounts_changed(webview: &WebView, addrs: Vec<String>) {
    let payload = Value::Array(addrs.into_iter().map(Value::String).collect());
    emit_provider_event(webview, "accountsChanged", payload);
}

pub fn emit_chain_changed(webview: &WebView, chain_id_hex: String) {
    emit_provider_event(webview, "chainChanged", Value::String(chain_id_hex));
}

pub fn emit_provider_event(webview: &WebView, event: &str, value: Value) {
    if let Err(err) = dispatch(
        webview,
        HostDispatchKind::ProviderEvent,
        ProviderEventPayload {
            event: event.to_string(),
            value,
        },
    ) {
        tracing::warn!(event, error = %err, "failed to dispatch provider event");
    }
}

pub fn emit_walletconnect_pairing(webview: &WebView, uri: &str, qr_svg: &str) {
    if let Err(err) = dispatch(
        webview,
        HostDispatchKind::WalletconnectPairing,
        WalletconnectPairingPayload {
            uri: uri.to_string(),
            qr_svg: qr_svg.to_string(),
        },
    ) {
        tracing::warn!(error = %err, "failed to dispatch walletconnect pairing payload");
    }
}

pub fn update_tabs(webview: &WebView, tabs: Vec<Value>, active_index: usize) -> Result<()> {
    dispatch(
        webview,
        HostDispatchKind::TabbarUpdate,
        TabbarUpdatePayload { tabs, active_index },
    )
}

pub fn update_rpc_status(webview: &WebView, webview_id: &str, pending_count: u32) -> Result<()> {
    dispatch(
        webview,
        HostDispatchKind::RpcStatus,
        RpcStatusPayload {
            webview_id: webview_id.to_string(),
            pending_count,
        },
    )
}
