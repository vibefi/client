use anyhow::{Result, anyhow};
use serde_json::Value;
use wry::WebView;

use crate::ipc_contract::IpcRequest;
use crate::state::{AppState, ProviderInfo, UserEvent, WalletBackend};
use crate::walletconnect::{HelperEvent, WalletConnectSession};

use super::rpc::parse_hex_u64;
use super::{emit_accounts_changed, emit_chain_changed, respond_err, respond_ok};

pub(super) fn handle_walletconnect_ipc(
    webview: &WebView,
    state: &AppState,
    webview_id: &str,
    req: &IpcRequest,
) -> Result<Option<Value>> {
    match req.method.as_str() {
        "eth_requestAccounts" => {
            let chain_id = state.wallet.lock().unwrap().chain.chain_id;
            tracing::info!(
                chain_id = format!("0x{:x}", chain_id),
                "walletconnect eth_requestAccounts received"
            );
            let bridge = state
                .walletconnect
                .lock()
                .unwrap()
                .as_ref()
                .ok_or_else(|| anyhow!("walletconnect bridge unavailable"))?
                .clone();
            let proxy = state.proxy.clone();
            let ipc_id = req.id;
            let wv_id = webview_id.to_string();

            std::thread::spawn(move || {
                let result = {
                    let mut bridge = bridge.lock().unwrap();
                    let proxy_for_events = proxy.clone();
                    bridge.connect_with_event_handler(chain_id, move |event| {
                        if event.event == "display_uri" {
                            if let Some(uri) = event.uri.clone() {
                                let qr_svg = event.qr_svg.clone().unwrap_or_default();
                                let _ = proxy_for_events
                                    .send_event(UserEvent::WalletConnectPairing { uri, qr_svg });
                            }
                        }
                    })
                };
                let mapped = result.map_err(|e| e.to_string());
                let _ = proxy.send_event(UserEvent::WalletConnectResult {
                    webview_id: wv_id,
                    ipc_id,
                    result: mapped,
                });
            });

            Ok(None)
        }
        "eth_accounts" => {
            let value =
                walletconnect_request(webview, state, req.method.as_str(), req.params.clone())?;
            let accounts = if let Some(arr) = value.as_array() {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            } else {
                vec![]
            };
            let mut ws = state.wallet.lock().unwrap();
            ws.authorized = !accounts.is_empty();
            ws.account = accounts.first().cloned();
            Ok(Some(value))
        }
        "eth_chainId" => {
            let value =
                walletconnect_request(webview, state, req.method.as_str(), req.params.clone())?;
            if let Some(chain_hex) = value.as_str() {
                if let Some(chain_id) = parse_hex_u64(chain_hex) {
                    let mut ws = state.wallet.lock().unwrap();
                    ws.chain.chain_id = chain_id;
                }
            }
            Ok(Some(value))
        }
        "net_version" => {
            let chain_hex =
                walletconnect_request(webview, state, "eth_chainId", Value::Array(vec![]))?;
            let chain_hex = chain_hex.as_str().unwrap_or("0x1");
            let chain_id = parse_hex_u64(chain_hex).unwrap_or(1);
            {
                let mut ws = state.wallet.lock().unwrap();
                ws.chain.chain_id = chain_id;
            }
            Ok(Some(Value::String(chain_id.to_string())))
        }
        "wallet_getProviderInfo" => {
            let ws = state.wallet.lock().unwrap();
            let info = ProviderInfo {
                name: "vibefi-walletconnect".to_string(),
                chain_id: format!("0x{:x}", ws.chain.chain_id),
                backend: "walletconnect",
                account: ws.account.clone(),
                walletconnect_uri: ws.walletconnect_uri.clone(),
            };
            Ok(Some(serde_json::to_value(info)?))
        }
        "wallet_switchEthereumChain" => {
            let value =
                walletconnect_request(webview, state, req.method.as_str(), req.params.clone())?;
            let chain_id_hex = req
                .params
                .get(0)
                .and_then(|v| v.get("chainId"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("invalid params for wallet_switchEthereumChain"))?;
            if let Some(chain_id) = parse_hex_u64(chain_id_hex) {
                let mut ws = state.wallet.lock().unwrap();
                ws.chain.chain_id = chain_id;
                emit_chain_changed(webview, format!("0x{:x}", chain_id));
            }
            Ok(Some(value))
        }
        _ => {
            walletconnect_request(webview, state, req.method.as_str(), req.params.clone()).map(Some)
        }
    }
}

fn walletconnect_request(
    webview: &WebView,
    state: &AppState,
    method: &str,
    params: Value,
) -> Result<Value> {
    let bridge = state
        .walletconnect
        .lock()
        .unwrap()
        .as_ref()
        .ok_or_else(|| anyhow!("walletconnect bridge unavailable"))?
        .clone();
    let mut bridge = bridge.lock().unwrap();
    let (result, events) = bridge.request(method, params)?;
    drop(bridge);

    apply_walletconnect_events(webview, state, &events);
    Ok(result)
}

fn apply_walletconnect_events(webview: &WebView, state: &AppState, events: &[HelperEvent]) {
    for event in events {
        apply_walletconnect_event(webview, state, event);
    }
}

fn apply_walletconnect_event(webview: &WebView, state: &AppState, event: &HelperEvent) {
    match event.event.as_str() {
        "display_uri" => {
            if let Some(uri) = event.uri.clone() {
                let qr_svg = event.qr_svg.clone().unwrap_or_default();
                tracing::info!("walletconnect pairing uri emitted");
                {
                    let mut ws = state.wallet.lock().unwrap();
                    ws.walletconnect_uri = Some(uri.clone());
                }
                let _ = state
                    .proxy
                    .send_event(UserEvent::WalletConnectPairing { uri, qr_svg });
            }
        }
        "accountsChanged" => {
            let accounts = event.accounts.clone().unwrap_or_default();
            {
                let mut ws = state.wallet.lock().unwrap();
                ws.authorized = !accounts.is_empty();
                ws.account = accounts.first().cloned();
            }
            emit_accounts_changed(webview, accounts);
        }
        "chainChanged" => {
            if let Some(chain_hex) = event.chain_id.clone() {
                if let Some(chain_id) = parse_hex_u64(&chain_hex) {
                    let mut ws = state.wallet.lock().unwrap();
                    ws.chain.chain_id = chain_id;
                }
                emit_chain_changed(webview, chain_hex);
            }
        }
        "disconnect" => {
            {
                let mut ws = state.wallet.lock().unwrap();
                ws.authorized = false;
                ws.account = None;
            }
            emit_accounts_changed(webview, Vec::new());
        }
        _ => {}
    }
}

pub fn handle_walletconnect_connect_result(
    webview: &WebView,
    state: &AppState,
    ipc_id: u64,
    result: Result<WalletConnectSession, String>,
) {
    match result {
        Ok(session) => {
            let chain_id = parse_hex_u64(&session.chain_id_hex)
                .unwrap_or(state.wallet.lock().unwrap().chain.chain_id);
            let accounts = session
                .accounts
                .iter()
                .map(|a| Value::String(a.clone()))
                .collect::<Vec<_>>();
            {
                let mut ws = state.wallet.lock().unwrap();
                ws.authorized = !session.accounts.is_empty();
                ws.account = session.accounts.first().cloned();
                ws.chain.chain_id = chain_id;
                ws.walletconnect_uri = None;
            }
            // Set backend to WalletConnect if not already set
            {
                let mut wb = state.wallet_backend.lock().unwrap();
                if wb.is_none() {
                    *wb = Some(WalletBackend::WalletConnect);
                }
            }
            if !session.accounts.is_empty() {
                emit_accounts_changed(webview, session.accounts.clone());
            }
            emit_chain_changed(webview, session.chain_id_hex.clone());
            let _ = state.proxy.send_event(UserEvent::CloseWalletSelector);
            tracing::info!(
                accounts = session.accounts.len(),
                "walletconnect eth_requestAccounts resolved"
            );
            if let Err(e) = respond_ok(webview, ipc_id, Value::Array(accounts)) {
                tracing::error!(error = %e, "walletconnect failed to send ok response");
            }
        }
        Err(msg) => {
            tracing::warn!(error = %msg, "walletconnect eth_requestAccounts failed");
            if let Err(e) = respond_err(webview, ipc_id, &msg) {
                tracing::error!(error = %e, "walletconnect failed to send error response");
            }
        }
    }
}
