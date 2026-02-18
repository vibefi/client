use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;
use wry::WebView;

use crate::ipc_contract::{IpcRequest, WalletSelectorMethod};
use crate::state::lock_or_err;
use crate::state::{AppState, UserEvent, WalletBackend};
use crate::walletconnect::{WalletConnectBridge, WalletConnectConfig, WalletConnectSession};
use crate::webview_manager::{AppWebViewKind, WebViewManager};

/// Handle IPC from the wallet selector tab.
pub(super) fn handle_wallet_selector_ipc(
    _webview: &WebView,
    manager: &WebViewManager,
    state: &AppState,
    webview_id: &str,
    req: &IpcRequest,
) -> Result<Option<Value>> {
    // Verify the request comes from the actual selector tab.
    if manager.app_kind_for_id(webview_id) != Some(AppWebViewKind::WalletSelector) {
        bail!("vibefi-wallet IPC only available to the wallet selector tab");
    }

    match req.wallet_selector_method() {
        Some(WalletSelectorMethod::GetCapabilities) => Ok(Some(serde_json::json!({
            "localSignerAvailable": local_signer_available(state),
            "localSignerRequiresPrivateKey": local_signer_requires_private_key(state),
        }))),
        Some(WalletSelectorMethod::ConnectLocal) => {
            tracing::info!("wallet-selector connecting local signer");
            let signer_hex = resolve_local_signer_hex(state, req)?;
            let signer: alloy_signer_local::PrivateKeySigner = signer_hex
                .parse()
                .context("failed to parse signing private key")?;
            let account = format!("0x{:x}", signer.address());

            // Store signer
            {
                let mut s = lock_or_err(&state.signer, "signer")?;
                *s = Some(std::sync::Arc::new(signer));
            }
            // Set backend
            {
                let mut wb = lock_or_err(&state.wallet_backend, "wallet_backend")?;
                *wb = Some(WalletBackend::Local);
            }
            // Update wallet state
            {
                let mut ws = lock_or_err(&state.wallet, "wallet")?;
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
            tracing::info!("wallet-selector connecting walletconnect");
            let resolved = state.resolved.as_ref();
            let project_id = resolved
                .and_then(|r| r.walletconnect_project_id.clone())
                .ok_or_else(|| {
                    anyhow!("WalletConnect requires walletConnect.projectId in config or VIBEFI_WC_PROJECT_ID env var")
                })?;
            let relay_url = resolved.and_then(|r| r.walletconnect_relay_url.clone());

            let bridge = WalletConnectBridge::spawn(WalletConnectConfig {
                project_id,
                relay_url,
            })
            .context("failed to initialize WalletConnect bridge")?;
            let bridge = std::sync::Arc::new(std::sync::Mutex::new(bridge));

            // Store bridge
            {
                let mut wc = lock_or_err(&state.walletconnect, "walletconnect")?;
                *wc = Some(bridge.clone());
            }

            let chain_id = lock_or_err(&state.wallet, "wallet")?.chain.chain_id;
            let proxy = state.proxy.clone();
            let ipc_id = req.id;
            let wv_id = webview_id.to_string();

            std::thread::spawn(move || {
                let result = {
                    let mut b = bridge.lock().expect("walletconnect_bridge");
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
            tracing::info!("wallet-selector connecting hardware wallet");
            let chain_id = lock_or_err(&state.wallet, "wallet")?.chain.chain_id;
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
                        tracing::error!(error = %e, "hardware failed to create tokio runtime");
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
                        tracing::info!(account, "hardware connected");

                        // Store hardware signer
                        {
                            let mut hs = hardware_signer.lock().expect("hardware_signer");
                            *hs = Some(device);
                        }
                        // Set backend
                        {
                            let mut wb = wallet_backend.lock().expect("wallet_backend");
                            *wb = Some(WalletBackend::Hardware);
                        }
                        // Update wallet state
                        {
                            let mut ws = wallet.lock().expect("wallet");
                            ws.authorized = true;
                            ws.account = Some(account.clone());
                        }

                        // Resolve pending connect if any
                        let pending: Vec<_> = pending_connect
                            .lock()
                            .expect("pending_connect")
                            .drain(..)
                            .collect();
                        for pc in pending {
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
                        tracing::warn!(error = %e, "hardware connection failed");
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

fn local_signer_available(state: &AppState) -> bool {
    is_test_network(state)
}

fn local_signer_requires_private_key(state: &AppState) -> bool {
    local_signer_available(state) && !has_configured_local_signer(state)
}

fn has_configured_local_signer(state: &AppState) -> bool {
    let resolved = state.resolved.as_ref();
    resolved.map(|r| r.local_network).unwrap_or(false)
        || resolved
            .and_then(|r| r.developer_private_key.as_ref())
            .is_some()
}

fn resolve_local_signer_hex(state: &AppState, req: &IpcRequest) -> Result<String> {
    if !is_test_network(state) {
        return Err(anyhow!(
            "Local signer is only available on test networks"
        ));
    }

    if let Some(private_key) = requested_local_private_key(req) {
        return Ok(private_key);
    }

    let resolved = state.resolved.as_ref();
    let is_local = resolved.map(|r| r.local_network).unwrap_or(false);
    let explicit_key = resolved.and_then(|r| r.developer_private_key.clone());
    if is_local {
        Ok(explicit_key.unwrap_or_else(|| crate::DEMO_PRIVKEY_HEX.to_string()))
    } else if let Some(key) = explicit_key {
        Ok(key)
    } else {
        Err(anyhow!(
            "No local signer configured for this test network. Enter a private key in the wallet selector."
        ))
    }
}

fn requested_local_private_key(req: &IpcRequest) -> Option<String> {
    req.params
        .as_array()
        .and_then(|params| params.first())
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn is_test_network(state: &AppState) -> bool {
    state
        .resolved
        .as_ref()
        .map(|resolved| resolved.test_network)
        .unwrap_or(false)
}

/// Resolve a pending `eth_requestAccounts` from a dapp tab by sending the
/// account list back to the original webview.
fn resolve_pending_connect(state: &AppState, accounts: Vec<String>) {
    let pending: Vec<_> = state
        .pending_connect
        .lock()
        .expect("pending_connect")
        .drain(..)
        .collect();
    for pc in pending {
        let _ = state.proxy.send_event(UserEvent::WalletConnectResult {
            webview_id: pc.webview_id,
            ipc_id: pc.ipc_id,
            result: Ok(WalletConnectSession {
                accounts: accounts.clone(),
                chain_id_hex: state.chain_id_hex(),
            }),
        });
    }
}
