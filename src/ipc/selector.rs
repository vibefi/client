use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;
use wry::WebView;

use crate::ipc_contract::{IpcRequest, WalletSelectorMethod};
use crate::state::{AppState, UserEvent, WalletBackend};
use crate::walletconnect::{WalletConnectBridge, WalletConnectConfig, WalletConnectSession};

/// Handle IPC from the wallet selector tab.
pub(super) fn handle_wallet_selector_ipc(
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

    match req.wallet_selector_method() {
        Some(WalletSelectorMethod::ConnectLocal) => {
            eprintln!("[wallet-selector] connecting local signer");
            let devnet = state.devnet.as_ref();
            let signer_hex = devnet
                .and_then(|ctx| ctx.config.developerPrivateKey.clone())
                .unwrap_or_else(|| crate::DEMO_PRIVKEY_HEX.to_string());
            let signer: alloy_signer_local::PrivateKeySigner = signer_hex
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
        Some(WalletSelectorMethod::ConnectWalletConnect) => {
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
        Some(WalletSelectorMethod::ConnectHardware) => {
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
        None => bail!("Unknown wallet selector method: {}", req.method),
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
