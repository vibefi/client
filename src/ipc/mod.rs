mod hardware;
mod local;
mod router;
mod rpc;
mod selector;
mod walletconnect;

use anyhow::Result;
use serde_json::Value;
use wry::WebView;

pub use router::handle_ipc;
pub use walletconnect::handle_walletconnect_connect_result;

/// Emit accountsChanged to all app webviews via the manager.
pub fn broadcast_accounts_changed(
    manager: &crate::webview_manager::WebViewManager,
    addrs: Vec<String>,
) {
    for entry in &manager.apps {
        emit_accounts_changed(&entry.webview, addrs.clone());
    }
}

/// Emit chainChanged to all app webviews via the manager.
pub fn broadcast_chain_changed(
    manager: &crate::webview_manager::WebViewManager,
    chain_id_hex: String,
) {
    for entry in &manager.apps {
        emit_chain_changed(&entry.webview, chain_id_hex.clone());
    }
}

pub fn respond_ok(webview: &WebView, id: u64, value: Value) -> Result<()> {
    crate::ui_bridge::respond_ok(webview, id, value)
}

pub fn respond_err(webview: &WebView, id: u64, message: &str) -> Result<()> {
    crate::ui_bridge::respond_err(webview, id, message)
}

pub fn emit_accounts_changed(webview: &WebView, addrs: Vec<String>) {
    crate::ui_bridge::emit_accounts_changed(webview, addrs);
}

pub fn emit_chain_changed(webview: &WebView, chain_id_hex: String) {
    crate::ui_bridge::emit_chain_changed(webview, chain_id_hex);
}
