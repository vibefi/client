use alloy_consensus::TypedTransaction;
use alloy_eips::eip2718::Encodable2718;
use alloy_network::TxSignerSync;
use alloy_primitives::{Address, B256, Signature};
use alloy_rpc_types_eth::TransactionRequest;
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;
use wry::WebView;

use crate::devnet::handle_launcher_ipc;
use crate::state::{AppState, IpcRequest, PendingConnect, ProviderInfo, UserEvent, WalletBackend};
use crate::ui_bridge;
use crate::walletconnect::{
    HelperEvent, WalletConnectBridge, WalletConnectConfig, WalletConnectSession,
};

/// Emit accountsChanged to all app webviews via the manager.
pub fn broadcast_accounts_changed(
    manager: &crate::webview_manager::WebViewManager,
    addrs: Vec<String>,
) {
    for entry in &manager.apps {
        ui_bridge::emit_accounts_changed(&entry.webview, addrs.clone());
    }
}

/// Emit chainChanged to all app webviews via the manager.
pub fn broadcast_chain_changed(
    manager: &crate::webview_manager::WebViewManager,
    chain_id_hex: String,
) {
    for entry in &manager.apps {
        ui_bridge::emit_chain_changed(&entry.webview, chain_id_hex.clone());
    }
}

pub fn handle_ipc(
    webview: &WebView,
    state: &AppState,
    webview_id: &str,
    msg: String,
) -> Result<()> {
    let req: IpcRequest = serde_json::from_str(&msg).context("invalid IPC JSON")?;

    // Handle vibefi-wallet IPC from the wallet selector tab.
    if matches!(req.provider_id.as_deref(), Some("vibefi-wallet")) {
        let result = handle_wallet_selector_ipc(webview, state, webview_id, &req);
        match result {
            Ok(Some(v)) => respond_ok(webview, req.id, v)?,
            Ok(None) => { /* response will be sent later */ }
            Err(e) => respond_err(webview, req.id, &e.to_string())?,
        }
        return Ok(());
    }

    if matches!(req.provider_id.as_deref(), Some("vibefi-launcher")) {
        if webview_id != "app-0" {
            bail!("launcher IPC is only available to the launcher webview");
        }
        let result = handle_launcher_ipc(webview, state, &req);
        match result {
            Ok(v) => respond_ok(webview, req.id, v)?,
            Err(e) => respond_err(webview, req.id, &e.to_string())?,
        }
        return Ok(());
    }

    let backend = state.get_wallet_backend();

    // If no wallet backend is chosen yet and the dapp calls eth_requestAccounts,
    // open the wallet selector tab and park the request.
    if backend.is_none() && req.method == "eth_requestAccounts" {
        {
            let mut pending = state.pending_connect.lock().unwrap();
            *pending = Some(PendingConnect {
                webview_id: webview_id.to_string(),
                ipc_id: req.id,
            });
        }
        let _ = state.proxy.send_event(UserEvent::OpenWalletSelector);
        // Response will be sent later once the user picks a wallet.
        return Ok(());
    }

    let result = match backend {
        Some(WalletBackend::Local) => handle_local_ipc(webview, state, &req).map(Some),
        Some(WalletBackend::WalletConnect) => {
            handle_walletconnect_ipc(webview, state, webview_id, &req)
        }
        Some(WalletBackend::Hardware) => handle_hardware_ipc(state, webview_id, &req),
        None => {
            // For methods other than eth_requestAccounts when no wallet is selected,
            // return sensible defaults.
            match req.method.as_str() {
                "eth_chainId" => Ok(Some(Value::String(state.chain_id_hex()))),
                "net_version" => {
                    let chain_id = state.wallet.lock().unwrap().chain.chain_id;
                    Ok(Some(Value::String(chain_id.to_string())))
                }
                "eth_accounts" => Ok(Some(Value::Array(vec![]))),
                "wallet_getProviderInfo" => {
                    let info = ProviderInfo {
                        name: "vibefi".to_string(),
                        chain_id: state.chain_id_hex(),
                        backend: "none",
                        account: None,
                        walletconnect_uri: None,
                    };
                    Ok(Some(serde_json::to_value(info)?))
                }
                _ => {
                    if state.devnet.is_some() && is_rpc_passthrough(req.method.as_str()) {
                        proxy_rpc(state, &req).map(Some)
                    } else {
                        Err(anyhow!(
                            "No wallet connected. Call eth_requestAccounts first."
                        ))
                    }
                }
            }
        }
    };

    match result {
        Ok(Some(v)) => respond_ok(webview, req.id, v)?,
        Ok(None) => { /* response will be sent later via UserEvent */ }
        Err(e) => respond_err(webview, req.id, &e.to_string())?,
    }

    Ok(())
}

/// Handle IPC from the wallet selector tab.
fn handle_wallet_selector_ipc(
    _webview: &WebView,
    state: &AppState,
    webview_id: &str,
    req: &IpcRequest,
) -> Result<Option<Value>> {
    // Verify the request comes from the actual selector tab.
    {
        let sel_id = state.selector_webview_id.lock().unwrap();
        if sel_id.as_deref() != Some(webview_id) {
            bail!("vibefi-wallet IPC only available to the wallet selector tab");
        }
    }

    match req.method.as_str() {
        "vibefi_connectLocal" => {
            eprintln!("[wallet-selector] connecting local signer");
            let devnet = state.devnet.as_ref();
            let signer_hex = devnet
                .and_then(|ctx| ctx.config.developerPrivateKey.clone())
                .unwrap_or_else(|| crate::DEMO_PRIVKEY_HEX.to_string());
            let signer: PrivateKeySigner = signer_hex
                .parse()
                .context("failed to parse signing private key")?;
            let account = format!("0x{:x}", signer.address());

            // Store signer
            {
                let mut s = state.signer.lock().unwrap();
                *s = Some(std::sync::Arc::new(signer));
            }
            // Set backend
            {
                let mut wb = state.wallet_backend.lock().unwrap();
                *wb = Some(WalletBackend::Local);
            }
            // Update wallet state
            {
                let mut ws = state.wallet.lock().unwrap();
                ws.authorized = true;
                ws.account = Some(account.clone());
            }

            // Resolve the pending eth_requestAccounts
            resolve_pending_connect(state, vec![account]);

            // Close the selector tab
            let _ = state.proxy.send_event(UserEvent::CloseWalletSelector);

            Ok(Some(Value::Bool(true)))
        }
        "vibefi_connectWalletConnect" => {
            eprintln!("[wallet-selector] connecting walletconnect");
            let project_id = state
                .wc_project_id
                .clone()
                .or_else(|| std::env::var("VIBEFI_WC_PROJECT_ID").ok())
                .or_else(|| std::env::var("WC_PROJECT_ID").ok())
                .ok_or_else(|| {
                    anyhow!("WalletConnect requires --wc-project-id or VIBEFI_WC_PROJECT_ID")
                })?;
            let relay_url = state
                .wc_relay_url
                .clone()
                .or_else(|| std::env::var("VIBEFI_WC_RELAY_URL").ok())
                .or_else(|| std::env::var("WC_RELAY_URL").ok());

            let bridge = WalletConnectBridge::spawn(WalletConnectConfig {
                project_id,
                relay_url,
            })
            .context("failed to initialize WalletConnect bridge")?;
            let bridge = std::sync::Arc::new(std::sync::Mutex::new(bridge));

            // Store bridge
            {
                let mut wc = state.walletconnect.lock().unwrap();
                *wc = Some(bridge.clone());
            }

            let chain_id = state.wallet.lock().unwrap().chain.chain_id;
            let proxy = state.proxy.clone();
            let ipc_id = req.id;
            let wv_id = webview_id.to_string();

            std::thread::spawn(move || {
                let result = {
                    let mut b = bridge.lock().unwrap();
                    let proxy_for_events = proxy.clone();
                    b.connect_with_event_handler(chain_id, move |event| {
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

            // Response will come later via WalletConnectResult event
            Ok(None)
        }
        "vibefi_connectHardware" => {
            eprintln!("[wallet-selector] connecting hardware wallet");
            let chain_id = state.wallet.lock().unwrap().chain.chain_id;
            let proxy = state.proxy.clone();
            let hardware_signer = state.hardware_signer.clone();
            let wallet_backend = state.wallet_backend.clone();
            let wallet = state.wallet.clone();
            let pending_connect = state.pending_connect.clone();
            let ipc_id = req.id;
            let wv_id = webview_id.to_string();
            let chain_id_hex = state.chain_id_hex();

            std::thread::spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        eprintln!("[hardware] failed to create tokio runtime: {e}");
                        let _ = proxy.send_event(UserEvent::HardwareSignResult {
                            webview_id: wv_id,
                            ipc_id,
                            result: Err(format!("runtime error: {e}")),
                        });
                        return;
                    }
                };

                match rt.block_on(crate::hardware::detect_and_connect(chain_id)) {
                    Ok(device) => {
                        let account = crate::hardware::get_address(&device);
                        eprintln!("[hardware] connected: {account}");

                        // Store hardware signer
                        {
                            let mut hs = hardware_signer.lock().unwrap();
                            *hs = Some(device);
                        }
                        // Set backend
                        {
                            let mut wb = wallet_backend.lock().unwrap();
                            *wb = Some(WalletBackend::Hardware);
                        }
                        // Update wallet state
                        {
                            let mut ws = wallet.lock().unwrap();
                            ws.authorized = true;
                            ws.account = Some(account.clone());
                        }

                        // Resolve pending connect if any
                        let pending = pending_connect.lock().unwrap().take();
                        if let Some(pc) = pending {
                            let _ = proxy.send_event(UserEvent::WalletConnectResult {
                                webview_id: pc.webview_id,
                                ipc_id: pc.ipc_id,
                                result: Ok(WalletConnectSession {
                                    accounts: vec![account.clone()],
                                    chain_id_hex: chain_id_hex.clone(),
                                }),
                            });
                        }

                        // Respond OK to the selector tab
                        let _ = proxy.send_event(UserEvent::HardwareSignResult {
                            webview_id: wv_id,
                            ipc_id,
                            result: Ok("true".to_string()),
                        });

                        // Close selector
                        let _ = proxy.send_event(UserEvent::CloseWalletSelector);
                    }
                    Err(e) => {
                        eprintln!("[hardware] connection failed: {e}");
                        let _ = proxy.send_event(UserEvent::HardwareSignResult {
                            webview_id: wv_id,
                            ipc_id,
                            result: Err(e.to_string()),
                        });
                    }
                }
            });

            // Response comes later via HardwareSignResult event
            Ok(None)
        }
        _ => bail!("Unknown wallet selector method: {}", req.method),
    }
}

/// Resolve a pending `eth_requestAccounts` from a dapp tab by sending the
/// account list back to the original webview.
fn resolve_pending_connect(state: &AppState, accounts: Vec<String>) {
    let pending = {
        let mut p = state.pending_connect.lock().unwrap();
        p.take()
    };
    if let Some(pc) = pending {
        // We can't directly access the webview here — send an event so the
        // main loop can find the right webview and respond.
        // Instead, we use the proxy to emit a WalletConnectResult-like event.
        // But actually we need to respond directly. We'll store the accounts
        // in wallet state (already done) and emit events that get picked up.
        // The actual response is handled via the proxy in the main event loop.
        let _ = state.proxy.send_event(UserEvent::WalletConnectResult {
            webview_id: pc.webview_id,
            ipc_id: pc.ipc_id,
            result: Ok(WalletConnectSession {
                accounts,
                chain_id_hex: state.chain_id_hex(),
            }),
        });
    }
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

            let tx_obj = req
                .params
                .get(0)
                .cloned()
                .ok_or_else(|| anyhow!("invalid params for eth_sendTransaction"))?;
            let tx_request = build_filled_tx_request(state, tx_obj)?;
            let mut tx = build_typed_tx(tx_request)?;
            let signer = state
                .local_signer()
                .ok_or_else(|| anyhow!("Local signer unavailable"))?;
            let sig = signer
                .sign_transaction_sync(&mut tx)
                .context("sign_transaction failed")?;
            let raw_tx_hex = encode_signed_typed_tx_hex(tx, sig);
            let tx_hash = send_raw_transaction(state, raw_tx_hex)?;
            Ok(Value::String(tx_hash))
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

fn handle_hardware_ipc(
    state: &AppState,
    webview_id: &str,
    req: &IpcRequest,
) -> Result<Option<Value>> {
    match req.method.as_str() {
        "eth_chainId" => Ok(Some(Value::String(state.chain_id_hex()))),
        "net_version" => {
            let chain_id = state.wallet.lock().unwrap().chain.chain_id;
            Ok(Some(Value::String(chain_id.to_string())))
        }
        "eth_accounts" | "eth_requestAccounts" => {
            let ws = state.wallet.lock().unwrap();
            if ws.authorized {
                if let Some(account) = ws.account.clone() {
                    Ok(Some(Value::Array(vec![Value::String(account)])))
                } else {
                    Ok(Some(Value::Array(vec![])))
                }
            } else {
                Ok(Some(Value::Array(vec![])))
            }
        }
        "wallet_getProviderInfo" => {
            let ws = state.wallet.lock().unwrap();
            let info = ProviderInfo {
                name: "vibefi-hardware".to_string(),
                chain_id: format!("0x{:x}", ws.chain.chain_id),
                backend: "hardware",
                account: ws.account.clone(),
                walletconnect_uri: None,
            };
            Ok(Some(serde_json::to_value(info)?))
        }
        "personal_sign" => {
            let msg = req
                .params
                .get(0)
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("invalid params for personal_sign"))?
                .to_string();
            let bytes = if let Some(b) = decode_0x_hex(&msg) {
                b
            } else {
                msg.as_bytes().to_vec()
            };

            let proxy = state.proxy.clone();
            let hardware_signer = state.hardware_signer.clone();
            let ipc_id = req.id;
            let wv_id = webview_id.to_string();

            std::thread::spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = proxy.send_event(UserEvent::HardwareSignResult {
                            webview_id: wv_id,
                            ipc_id,
                            result: Err(format!("runtime error: {e}")),
                        });
                        return;
                    }
                };
                let hs = hardware_signer.lock().unwrap();
                let device = match hs.as_ref() {
                    Some(d) => d,
                    None => {
                        let _ = proxy.send_event(UserEvent::HardwareSignResult {
                            webview_id: wv_id,
                            ipc_id,
                            result: Err("Hardware wallet not connected".to_string()),
                        });
                        return;
                    }
                };
                let result = rt
                    .block_on(crate::hardware::sign_message(device, &bytes))
                    .map_err(format_hardware_error);
                drop(hs);
                let _ = proxy.send_event(UserEvent::HardwareSignResult {
                    webview_id: wv_id,
                    ipc_id,
                    result,
                });
            });

            Ok(None) // deferred
        }
        "eth_signTypedData_v4" => {
            let typed_data_json = req
                .params
                .get(1)
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("invalid params for eth_signTypedData_v4"))?
                .to_string();

            let proxy = state.proxy.clone();
            let hardware_signer = state.hardware_signer.clone();
            let ipc_id = req.id;
            let wv_id = webview_id.to_string();

            std::thread::spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = proxy.send_event(UserEvent::HardwareSignResult {
                            webview_id: wv_id,
                            ipc_id,
                            result: Err(format!("runtime error: {e}")),
                        });
                        return;
                    }
                };
                let hash = alloy_primitives::keccak256(typed_data_json.as_bytes());
                let hs = hardware_signer.lock().unwrap();
                let device = match hs.as_ref() {
                    Some(d) => d,
                    None => {
                        let _ = proxy.send_event(UserEvent::HardwareSignResult {
                            webview_id: wv_id,
                            ipc_id,
                            result: Err("Hardware wallet not connected".to_string()),
                        });
                        return;
                    }
                };
                let result = rt
                    .block_on(crate::hardware::sign_hash(device, hash.into()))
                    .map_err(format_hardware_error);
                drop(hs);
                let _ = proxy.send_event(UserEvent::HardwareSignResult {
                    webview_id: wv_id,
                    ipc_id,
                    result,
                });
            });

            Ok(None) // deferred
        }
        "eth_sendTransaction" => {
            let ws = state.wallet.lock().unwrap();
            if !ws.authorized {
                return Err(anyhow!("Unauthorized: call eth_requestAccounts first"));
            }
            drop(ws);

            let tx_obj = req
                .params
                .get(0)
                .cloned()
                .ok_or_else(|| anyhow!("invalid params for eth_sendTransaction"))?;
            let tx_request = build_filled_tx_request(state, tx_obj)?;
            let mut tx = build_typed_tx(tx_request)?;

            // Sign and broadcast the typed transaction via the connected hardware device.
            let proxy = state.proxy.clone();
            let hardware_signer = state.hardware_signer.clone();
            let state_for_rpc = state.clone();
            let ipc_id = req.id;
            let wv_id = webview_id.to_string();

            std::thread::spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = proxy.send_event(UserEvent::HardwareSignResult {
                            webview_id: wv_id,
                            ipc_id,
                            result: Err(format!("runtime error: {e}")),
                        });
                        return;
                    }
                };

                let hs = hardware_signer.lock().unwrap();
                let device = match hs.as_ref() {
                    Some(d) => d,
                    None => {
                        let _ = proxy.send_event(UserEvent::HardwareSignResult {
                            webview_id: wv_id,
                            ipc_id,
                            result: Err("Hardware wallet not connected".to_string()),
                        });
                        return;
                    }
                };

                let sign_result = rt
                    .block_on(crate::hardware::sign_transaction(device, &mut tx))
                    .map_err(format_hardware_error);
                drop(hs);

                let result = match sign_result {
                    Ok(sig) => {
                        let raw_tx_hex = encode_signed_typed_tx_hex(tx, sig);
                        send_raw_transaction(&state_for_rpc, raw_tx_hex).map_err(|e| e.to_string())
                    }
                    Err(e) => Err(e),
                };

                let _ = proxy.send_event(UserEvent::HardwareSignResult {
                    webview_id: wv_id,
                    ipc_id,
                    result,
                });
            });

            Ok(None) // deferred
        }
        _ => {
            if state.devnet.is_some() && is_rpc_passthrough(req.method.as_str()) {
                proxy_rpc(state, req).map(Some)
            } else {
                Err(anyhow!("Unsupported method: {}", req.method))
            }
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
                println!("[WalletConnect] pairing uri: {uri}");
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
            eprintln!(
                "[walletconnect] eth_requestAccounts resolved ({} account(s))",
                session.accounts.len()
            );
            if let Err(e) = respond_ok(webview, ipc_id, Value::Array(accounts)) {
                eprintln!("[walletconnect] failed to send ok response: {e}");
            }
        }
        Err(msg) => {
            eprintln!("[walletconnect] eth_requestAccounts failed: {msg}");
            if let Err(e) = respond_err(webview, ipc_id, &msg) {
                eprintln!("[walletconnect] failed to send error response: {e}");
            }
        }
    }
}

pub fn respond_ok(webview: &WebView, id: u64, value: Value) -> Result<()> {
    ui_bridge::respond_ok(webview, id, value)
}

pub fn respond_err(webview: &WebView, id: u64, message: &str) -> Result<()> {
    ui_bridge::respond_err(webview, id, message)
}

pub fn emit_accounts_changed(webview: &WebView, addrs: Vec<String>) {
    ui_bridge::emit_accounts_changed(webview, addrs);
}

pub fn emit_chain_changed(webview: &WebView, chain_id_hex: String) {
    ui_bridge::emit_chain_changed(webview, chain_id_hex);
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

fn rpc_request(state: &AppState, method: &str, params: Value) -> Result<Value> {
    if state.devnet.is_none() {
        bail!("RPC backend unavailable. Run with --devnet to send transactions.");
    }

    let req = IpcRequest {
        id: 0,
        provider_id: None,
        method: method.to_string(),
        params,
    };
    proxy_rpc(state, &req)
}

fn rpc_quantity_u64(state: &AppState, method: &str, params: Value) -> Result<u64> {
    let v = rpc_request(state, method, params)?;
    let s = v
        .as_str()
        .ok_or_else(|| anyhow!("{} returned non-string quantity", method))?;
    parse_hex_u64(s).ok_or_else(|| anyhow!("{} returned invalid quantity", method))
}

fn rpc_quantity_u128(state: &AppState, method: &str, params: Value) -> Result<u128> {
    let v = rpc_request(state, method, params)?;
    let s = v
        .as_str()
        .ok_or_else(|| anyhow!("{} returned non-string quantity", method))?;
    parse_hex_u128(s).ok_or_else(|| anyhow!("{} returned invalid quantity", method))
}

fn connected_sender(state: &AppState) -> Result<Address> {
    let account = state
        .account()
        .ok_or_else(|| anyhow!("No connected account available for transaction sending"))?;
    account
        .parse::<Address>()
        .with_context(|| format!("invalid connected account address: {account}"))
}

fn build_filled_tx_request(state: &AppState, tx_obj: Value) -> Result<TransactionRequest> {
    let mut tx: TransactionRequest =
        serde_json::from_value(tx_obj).context("invalid eth_sendTransaction object")?;
    let sender = connected_sender(state)?;

    // Enforce backend account ownership for signing.
    if let Some(from) = tx.from {
        if from != sender {
            bail!(
                "Transaction 'from' ({:#x}) does not match connected account ({:#x})",
                from,
                sender
            );
        }
    } else {
        tx.from = Some(sender);
    }

    if tx.chain_id.is_none() {
        tx.chain_id = Some(state.wallet.lock().unwrap().chain.chain_id);
    }

    if tx.nonce.is_none() {
        tx.nonce = Some(rpc_quantity_u64(
            state,
            "eth_getTransactionCount",
            Value::Array(vec![
                Value::String(format!("{:#x}", sender)),
                Value::String("pending".to_string()),
            ]),
        )?);
    }

    if tx.gas.is_none() {
        let estimate_obj =
            serde_json::to_value(&tx).context("failed to encode tx for estimateGas")?;
        tx.gas = Some(rpc_quantity_u64(
            state,
            "eth_estimateGas",
            Value::Array(vec![estimate_obj]),
        )?);
    }

    // Fill fee defaults when omitted by dapp.
    let has_legacy_fee = tx.gas_price.is_some();
    let has_1559_fee = tx.max_fee_per_gas.is_some() || tx.max_priority_fee_per_gas.is_some();

    if !has_legacy_fee && !has_1559_fee {
        let gas_price = rpc_quantity_u128(state, "eth_gasPrice", Value::Array(vec![]))?;
        let priority = rpc_quantity_u128(state, "eth_maxPriorityFeePerGas", Value::Array(vec![]))
            .unwrap_or(gas_price);
        tx.max_fee_per_gas = Some(gas_price);
        tx.max_priority_fee_per_gas = Some(priority.min(gas_price));
    } else if has_1559_fee {
        if tx.max_fee_per_gas.is_none() {
            let gas_price = rpc_quantity_u128(state, "eth_gasPrice", Value::Array(vec![]))?;
            tx.max_fee_per_gas = Some(gas_price);
        }
        if tx.max_priority_fee_per_gas.is_none() {
            let gas_price = tx.max_fee_per_gas.unwrap_or(0);
            let priority =
                rpc_quantity_u128(state, "eth_maxPriorityFeePerGas", Value::Array(vec![]))
                    .unwrap_or(gas_price);
            tx.max_priority_fee_per_gas = Some(priority.min(gas_price));
        }
        // Avoid conflicting legacy + 1559 fee fields.
        tx.gas_price = None;
    } else {
        // Legacy path: keep only gasPrice.
        tx.max_fee_per_gas = None;
        tx.max_priority_fee_per_gas = None;
    }

    Ok(tx)
}

fn build_typed_tx(mut tx: TransactionRequest) -> Result<TypedTransaction> {
    tx.trim_conflicting_keys();
    tx.build_typed_tx().map_err(|req| {
        let details = match req.missing_keys() {
            Ok(ty) => format!("transaction is not buildable for {:?}", ty),
            Err((ty, missing)) => format!("{:?} missing: {}", ty, missing.join(", ")),
        };
        anyhow!("unable to build signable transaction: {details}")
    })
}

fn encode_signed_typed_tx_hex(tx: TypedTransaction, signature: Signature) -> String {
    let envelope = tx.into_envelope(signature);
    format!("0x{}", hex::encode(envelope.encoded_2718()))
}

fn send_raw_transaction(state: &AppState, raw_tx_hex: String) -> Result<String> {
    let v = rpc_request(
        state,
        "eth_sendRawTransaction",
        Value::Array(vec![Value::String(raw_tx_hex)]),
    )?;
    let hash = v
        .as_str()
        .ok_or_else(|| anyhow!("eth_sendRawTransaction returned non-string hash"))?;
    Ok(hash.to_string())
}

fn format_hardware_error(err: anyhow::Error) -> String {
    let msg = format!("{err:#}");

    // Common Ledger policy/user-action errors during tx signing.
    if msg.contains("APDU_CODE_CONDITIONS_NOT_SATISFIED")
        || msg.contains("APDU_CODE_INVALID_DATA")
        || msg.contains("APDU_CODE_COMMAND_NOT_ALLOWED")
        || msg.contains("APDU_CODE_INS_NOT_SUPPORTED")
    {
        return format!(
            "{}\nHint: On Ledger, open the Ethereum app and enable 'Blind signing' in Settings, then approve the transaction on device.",
            msg
        );
    }

    msg
}

fn parse_hex_u64(s: &str) -> Option<u64> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let s = if s.is_empty() { "0" } else { s };
    u64::from_str_radix(s, 16).ok()
}

fn parse_hex_u128(s: &str) -> Option<u128> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let s = if s.is_empty() { "0" } else { s };
    u128::from_str_radix(s, 16).ok()
}

fn decode_0x_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.strip_prefix("0x")?;
    if s.len() % 2 != 0 {
        return None;
    }
    hex::decode(s).ok()
}
