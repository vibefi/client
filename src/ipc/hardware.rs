use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::ipc_contract::IpcRequest;
use crate::state::{AppState, ProviderInfo, UserEvent};

use super::rpc::{
    build_filled_tx_request, build_typed_tx, decode_0x_hex, encode_signed_typed_tx_hex,
    is_rpc_passthrough, proxy_rpc, send_raw_transaction,
};

pub(super) fn handle_hardware_ipc(
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
