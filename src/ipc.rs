use alloy_primitives::B256;
use alloy_signer::SignerSync;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use wry::WebView;

use crate::devnet::handle_launcher_ipc;
use crate::state::{AppState, IpcRequest, ProviderInfo, UserEvent, WalletBackend};
use crate::walletconnect::{HelperEvent, WalletConnectSession};

/// Emit accountsChanged to all app webviews via the manager.
pub fn broadcast_accounts_changed(manager: &crate::webview_manager::WebViewManager, addrs: Vec<String>) {
    let arr: Vec<serde_json::Value> = addrs.into_iter().map(serde_json::Value::String).collect();
    let payload = serde_json::Value::Array(arr);
    let js = format!("window.__WryEthereumEmit('accountsChanged', {});", payload);
    manager.broadcast_to_apps(&js);
}

/// Emit chainChanged to all app webviews via the manager.
pub fn broadcast_chain_changed(manager: &crate::webview_manager::WebViewManager, chain_id_hex: String) {
    let payload = serde_json::Value::String(chain_id_hex);
    let js = format!("window.__WryEthereumEmit('chainChanged', {});", payload);
    manager.broadcast_to_apps(&js);
}

pub fn handle_ipc(webview: &WebView, state: &AppState, webview_id: &str, msg: String) -> Result<()> {
    let req: IpcRequest = serde_json::from_str(&msg).context("invalid IPC JSON")?;
    if matches!(req.provider_id.as_deref(), Some("vibefi-launcher")) {
        let result = handle_launcher_ipc(webview, state, &req);
        match result {
            Ok(v) => respond_ok(webview, req.id, v)?,
            Err(e) => respond_err(webview, req.id, &e.to_string())?,
        }
        return Ok(());
    }

    let result = match state.wallet_backend {
        WalletBackend::Local => handle_local_ipc(webview, state, &req).map(Some),
        WalletBackend::WalletConnect => handle_walletconnect_ipc(webview, state, webview_id, &req),
    };

    match result {
        Ok(Some(v)) => respond_ok(webview, req.id, v)?,
        Ok(None) => { /* response will be sent later via UserEvent */ }
        Err(e) => respond_err(webview, req.id, &e.to_string())?,
    }

    Ok(())
}

fn handle_local_ipc(webview: &WebView, state: &AppState, req: &IpcRequest) -> Result<Value> {
    match req.method.as_str() {
        "eth_chainId" => Ok(Value::String(state.chain_id_hex())),
        "net_version" => {
            let chain_id = state.wallet.lock().unwrap().chain.chain_id;
            Ok(Value::String(chain_id.to_string()))
        }
        "eth_accounts" => {
            let ws = state.wallet.lock().unwrap();
            if ws.authorized {
                if let Some(account) = ws.account.clone().or_else(|| state.local_signer_address()) {
                    Ok(Value::Array(vec![Value::String(account)]))
                } else {
                    Ok(Value::Array(vec![]))
                }
            } else {
                Ok(Value::Array(vec![]))
            }
        }
        "eth_requestAccounts" => {
            let account = state
                .local_signer_address()
                .ok_or_else(|| anyhow!("Local signer unavailable"))?;
            {
                let mut ws = state.wallet.lock().unwrap();
                ws.authorized = true;
                ws.account = Some(account.clone());
            }
            emit_accounts_changed(webview, vec![account.clone()]);
            Ok(Value::Array(vec![Value::String(account)]))
        }
        "wallet_switchEthereumChain" => {
            let chain_id_hex = req
                .params
                .get(0)
                .and_then(|v| v.get("chainId"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("invalid params for wallet_switchEthereumChain"))?;
            let chain_id = parse_hex_u64(chain_id_hex).ok_or_else(|| anyhow!("invalid chainId"))?;

            if !matches!(chain_id, 1 | 11155111 | 31337) {
                return Err(anyhow!("Unsupported chainId in local demo wallet"));
            }

            {
                let mut ws = state.wallet.lock().unwrap();
                ws.chain.chain_id = chain_id;
            }
            let chain_hex = format!("0x{:x}", chain_id);
            emit_chain_changed(webview, chain_hex);
            Ok(Value::Null)
        }
        "personal_sign" => {
            let msg = req
                .params
                .get(0)
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("invalid params for personal_sign"))?;
            let bytes = if let Some(b) = decode_0x_hex(msg) {
                b
            } else {
                msg.as_bytes().to_vec()
            };

            let signer = state
                .local_signer()
                .ok_or_else(|| anyhow!("Local signer unavailable"))?;
            let sig = signer
                .sign_message_sync(&bytes)
                .context("sign_message failed")?;
            Ok(Value::String(format!("0x{}", hex::encode(sig.as_bytes()))))
        }
        "eth_signTypedData_v4" => {
            let typed_data_json = req
                .params
                .get(1)
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("invalid params for eth_signTypedData_v4"))?;
            let hash = alloy_primitives::keccak256(typed_data_json.as_bytes());
            let signer = state
                .local_signer()
                .ok_or_else(|| anyhow!("Local signer unavailable"))?;
            let sig = signer
                .sign_hash_sync(&B256::from(hash))
                .context("sign_hash failed")?;
            Ok(Value::String(format!("0x{}", hex::encode(sig.as_bytes()))))
        }
        "eth_sendTransaction" => {
            let ws = state.wallet.lock().unwrap();
            if !ws.authorized {
                return Err(anyhow!("Unauthorized: call eth_requestAccounts first"));
            }
            drop(ws);

            if state.devnet.is_some() {
                let mut tx_obj = req
                    .params
                    .get(0)
                    .cloned()
                    .ok_or_else(|| anyhow!("invalid params for eth_sendTransaction"))?;
                if tx_obj.get("from").is_none() {
                    if let Some(account) = state.account() {
                        if let Some(obj) = tx_obj.as_object_mut() {
                            obj.insert("from".to_string(), Value::String(account));
                        }
                    }
                }

                let modified_req = IpcRequest {
                    id: req.id,
                    provider_id: req.provider_id.clone(),
                    method: req.method.clone(),
                    params: Value::Array(vec![tx_obj]),
                };
                proxy_rpc(state, &modified_req)
            } else {
                let tx_obj = req
                    .params
                    .get(0)
                    .cloned()
                    .ok_or_else(|| anyhow!("invalid params for eth_sendTransaction"))?;
                let canonical = serde_json::to_vec(&tx_obj).context("tx json encode")?;
                let digest = alloy_primitives::keccak256(&canonical);
                let signer = state
                    .local_signer()
                    .ok_or_else(|| anyhow!("Local signer unavailable"))?;
                let sig = signer
                    .sign_hash_sync(&B256::from(digest))
                    .context("sign_hash failed")?;
                let tx_hash = alloy_primitives::keccak256(sig.as_bytes());
                Ok(Value::String(format!("0x{}", hex::encode(tx_hash))))
            }
        }
        "wallet_getProviderInfo" => {
            let ws = state.wallet.lock().unwrap();
            let info = ProviderInfo {
                name: "vibefi-local-wallet".to_string(),
                chain_id: state.chain_id_hex(),
                backend: "local",
                account: ws.account.clone().or_else(|| state.local_signer_address()),
                walletconnect_uri: None,
            };
            Ok(serde_json::to_value(info)?)
        }
        _ => {
            if state.devnet.is_some() && is_rpc_passthrough(req.method.as_str()) {
                proxy_rpc(state, req)
            } else {
                Err(anyhow!("Unsupported method: {}", req.method))
            }
        }
    }
}

fn handle_walletconnect_ipc(
    _webview: &WebView,
    state: &AppState,
    webview_id: &str,
    req: &IpcRequest,
) -> Result<Option<Value>> {
    match req.method.as_str() {
        "eth_requestAccounts" => {
            let chain_id = state.wallet.lock().unwrap().chain.chain_id;
            eprintln!(
                "[walletconnect] eth_requestAccounts received (chain=0x{:x})",
                chain_id
            );
            let bridge = state
                .walletconnect
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
                                    .send_event(UserEvent::WalletConnectOverlay { uri, qr_svg });
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
                walletconnect_request(_webview, state, req.method.as_str(), req.params.clone())?;
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
                walletconnect_request(_webview, state, req.method.as_str(), req.params.clone())?;
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
                walletconnect_request(_webview, state, "eth_chainId", Value::Array(vec![]))?;
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
                walletconnect_request(_webview, state, req.method.as_str(), req.params.clone())?;
            let chain_id_hex = req
                .params
                .get(0)
                .and_then(|v| v.get("chainId"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("invalid params for wallet_switchEthereumChain"))?;
            if let Some(chain_id) = parse_hex_u64(chain_id_hex) {
                let mut ws = state.wallet.lock().unwrap();
                ws.chain.chain_id = chain_id;
                emit_chain_changed(_webview, format!("0x{:x}", chain_id));
            }
            Ok(Some(value))
        }
        _ => walletconnect_request(_webview, state, req.method.as_str(), req.params.clone())
            .map(Some),
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
        .as_ref()
        .ok_or_else(|| anyhow!("walletconnect bridge unavailable"))?;
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
                println!("[WalletConnect] pairing uri: {uri}");
                {
                    let mut ws = state.wallet.lock().unwrap();
                    ws.walletconnect_uri = Some(uri.clone());
                }
                // Route to the wallet overlay webview via event loop
                let _ = state
                    .proxy
                    .send_event(UserEvent::WalletConnectOverlay { uri, qr_svg });
            }
        }
        "accountsChanged" => {
            let accounts = event.accounts.clone().unwrap_or_default();
            {
                let mut ws = state.wallet.lock().unwrap();
                ws.authorized = !accounts.is_empty();
                ws.account = accounts.first().cloned();
            }
            if !accounts.is_empty() {
                let _ = state.proxy.send_event(UserEvent::HideWalletOverlay);
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
            if !session.accounts.is_empty() {
                emit_accounts_changed(webview, session.accounts.clone());
            }
            emit_chain_changed(webview, session.chain_id_hex.clone());
            let _ = state.proxy.send_event(UserEvent::HideWalletOverlay);
            eprintln!(
                "[walletconnect] eth_requestAccounts resolved ({} account(s))",
                session.accounts.len()
            );
            if let Err(e) = respond_ok(webview, ipc_id, Value::Array(accounts)) {
                eprintln!("[walletconnect] failed to send ok response: {e}");
            }
        }
        Err(msg) => {
            let _ = state.proxy.send_event(UserEvent::HideWalletOverlay);
            eprintln!("[walletconnect] eth_requestAccounts failed: {msg}");
            if let Err(e) = respond_err(webview, ipc_id, &msg) {
                eprintln!("[walletconnect] failed to send error response: {e}");
            }
        }
    }
}

pub fn respond_ok(webview: &WebView, id: u64, value: Value) -> Result<()> {
    let js = format!("window.__WryEthereumResolve({}, {}, null);", id, value);
    webview.evaluate_script(&js)?;
    Ok(())
}

pub fn respond_err(webview: &WebView, id: u64, message: &str) -> Result<()> {
    let err = serde_json::json!({
        "code": -32601,
        "message": message,
    });
    let js = format!("window.__WryEthereumResolve({}, null, {});", id, err);
    webview.evaluate_script(&js)?;
    Ok(())
}

pub fn emit_accounts_changed(webview: &WebView, addrs: Vec<String>) {
    let arr = addrs.into_iter().map(Value::String).collect::<Vec<_>>();
    let payload = Value::Array(arr);
    let js = format!("window.__WryEthereumEmit('accountsChanged', {});", payload);
    let _ = webview.evaluate_script(&js);
}

pub fn emit_chain_changed(webview: &WebView, chain_id_hex: String) {
    let payload = Value::String(chain_id_hex);
    let js = format!("window.__WryEthereumEmit('chainChanged', {});", payload);
    let _ = webview.evaluate_script(&js);
}

fn is_rpc_passthrough(method: &str) -> bool {
    matches!(
        method,
        "eth_blockNumber"
            | "eth_getBlockByNumber"
            | "eth_getBlockByHash"
            | "eth_getBalance"
            | "eth_getCode"
            | "eth_getLogs"
            | "eth_call"
            | "eth_estimateGas"
            | "eth_gasPrice"
            | "eth_feeHistory"
            | "eth_maxPriorityFeePerGas"
            | "eth_getTransactionReceipt"
            | "eth_getTransactionByHash"
            | "eth_getStorageAt"
            | "eth_getTransactionCount"
            | "eth_sendRawTransaction"
    )
}

fn proxy_rpc(state: &AppState, req: &IpcRequest) -> Result<Value> {
    let devnet = state
        .devnet
        .as_ref()
        .ok_or_else(|| anyhow!("Devnet not configured"))?;
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": req.method,
        "params": req.params,
    });

    println!(
        "[RPC] -> {} params={}",
        req.method,
        serde_json::to_string(&req.params).unwrap_or_default()
    );

    let res = devnet
        .http
        .post(&devnet.rpc_url)
        .json(&payload)
        .send()
        .context("rpc request failed")?;
    let v: Value = res.json().context("rpc decode failed")?;

    let result_str = v
        .get("result")
        .map(|r| {
            let s = r.to_string();
            if s.len() > 200 {
                format!("{}...", &s[..200])
            } else {
                s
            }
        })
        .unwrap_or_else(|| "null".to_string());

    if let Some(err) = v.get("error") {
        println!("[RPC] <- {} ERROR: {}", req.method, err);
        bail!("rpc error: {}", err);
    }

    println!("[RPC] <- {} result={}", req.method, result_str);
    Ok(v.get("result").cloned().unwrap_or(Value::Null))
}

fn parse_hex_u64(s: &str) -> Option<u64> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    u64::from_str_radix(s, 16).ok()
}

fn decode_0x_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.strip_prefix("0x")?;
    if s.len() % 2 != 0 {
        return None;
    }
    hex::decode(s).ok()
}
