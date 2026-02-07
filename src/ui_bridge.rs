use anyhow::Result;
use serde_json::{Value, json};
use wry::WebView;

fn dispatch(webview: &WebView, kind: &str, payload: Value) -> Result<()> {
    let envelope = json!({
        "kind": kind,
        "payload": payload,
    });
    let script = format!("window.__VibefiHostDispatch({});", envelope);
    webview.evaluate_script(&script)?;
    Ok(())
}

pub fn respond_ok(webview: &WebView, id: u64, value: Value) -> Result<()> {
    dispatch(
        webview,
        "rpcResponse",
        json!({
            "id": id,
            "result": value,
            "error": Value::Null,
        }),
    )
}

pub fn respond_err(webview: &WebView, id: u64, message: &str) -> Result<()> {
    dispatch(
        webview,
        "rpcResponse",
        json!({
            "id": id,
            "result": Value::Null,
            "error": {
                "code": -32601,
                "message": message,
            },
        }),
    )
}

pub fn emit_accounts_changed(webview: &WebView, addrs: Vec<String>) {
    let payload = Value::Array(addrs.into_iter().map(Value::String).collect());
    let _ = dispatch(
        webview,
        "providerEvent",
        json!({
            "event": "accountsChanged",
            "value": payload,
        }),
    );
}

pub fn emit_chain_changed(webview: &WebView, chain_id_hex: String) {
    let _ = dispatch(
        webview,
        "providerEvent",
        json!({
            "event": "chainChanged",
            "value": chain_id_hex,
        }),
    );
}

pub fn emit_walletconnect_pairing(webview: &WebView, uri: &str, qr_svg: &str) {
    let _ = dispatch(
        webview,
        "walletconnectPairing",
        json!({
            "uri": uri,
            "qrSvg": qr_svg,
        }),
    );
}

pub fn update_tabs(webview: &WebView, tabs: Vec<Value>, active_index: usize) -> Result<()> {
    dispatch(
        webview,
        "tabbarUpdate",
        json!({
            "tabs": tabs,
            "activeIndex": active_index,
        }),
    )
}
