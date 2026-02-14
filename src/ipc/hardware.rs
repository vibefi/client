use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::ipc_contract::IpcRequest;
use crate::state::{AppState, ProviderInfo, UserEvent};

use super::rpc::{
    build_filled_tx_request, build_typed_tx, decode_0x_hex, encode_signed_typed_tx_hex,
    send_raw_transaction,
};
use super::try_spawn_rpc_passthrough;

pub(super) fn handle_hardware_ipc(
    state: &AppState,
    webview_id: &str,
    req: &IpcRequest,
) -> Result<Option<Value>> {
    if let Some(value) = super::network_identity_response(state, req.method.as_str()) {
        return Ok(Some(value));
    }

    match req.method.as_str() {
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
            tracing::debug!(
                webview_id,
                ipc_id = req.id,
                "hardware personal_sign request"
            );

            spawn_hardware_async(state, webview_id, req.id, move |rt, hardware_signer| {
                with_connected_hardware_device(hardware_signer, |device| {
                    rt.block_on(crate::hardware::sign_message(device, &bytes))
                        .map_err(format_hardware_error)
                })
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
            tracing::debug!(
                webview_id,
                ipc_id = req.id,
                "hardware eth_signTypedData_v4 request"
            );

            spawn_hardware_async(state, webview_id, req.id, move |rt, hardware_signer| {
                let hash = alloy_primitives::keccak256(typed_data_json.as_bytes());
                with_connected_hardware_device(hardware_signer, |device| {
                    rt.block_on(crate::hardware::sign_hash(device, hash.into()))
                        .map_err(format_hardware_error)
                })
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

            // Sign and broadcast the typed transaction via the connected hardware device.
            let state_for_rpc = state.clone();
            let ipc_id = req.id;
            tracing::info!(
                webview_id,
                ipc_id,
                "hardware spawning eth_sendTransaction worker"
            );

            spawn_hardware_async(state, webview_id, ipc_id, move |rt, hardware_signer| {
                // Build and fill the tx request inside the thread to avoid blocking
                // the main event loop with the 4-5 sequential RPC fill calls.
                let tx_request =
                    build_filled_tx_request(&state_for_rpc, tx_obj).map_err(|e| e.to_string())?;
                let mut tx = build_typed_tx(tx_request).map_err(|e| e.to_string())?;

                let sig = with_connected_hardware_device(hardware_signer, |device| {
                    rt.block_on(crate::hardware::sign_transaction(device, &mut tx))
                        .map_err(format_hardware_error)
                })?;

                let raw_tx_hex = encode_signed_typed_tx_hex(tx, sig);
                send_raw_transaction(&state_for_rpc, raw_tx_hex).map_err(|e| e.to_string())
            });

            Ok(None) // deferred
        }
        _ => {
            if try_spawn_rpc_passthrough(state, webview_id, req) {
                Ok(None)
            } else {
                Err(anyhow!("Unsupported method: {}", req.method))
            }
        }
    }
}

fn spawn_hardware_async<F>(state: &AppState, webview_id: &str, ipc_id: u64, task: F)
where
    F: FnOnce(
            &tokio::runtime::Runtime,
            &std::sync::Arc<std::sync::Mutex<Option<crate::hardware::HardwareDevice>>>,
        ) -> std::result::Result<String, String>
        + Send
        + 'static,
{
    let proxy = state.proxy.clone();
    let hardware_signer = state.hardware_signer.clone();
    let wv_id = webview_id.to_string();
    tracing::debug!(webview_id, ipc_id, "spawning hardware async worker");

    std::thread::spawn(move || {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("runtime error: {e}"))
            .and_then(|rt| task(&rt, &hardware_signer));

        if let Err(err) = &result {
            tracing::warn!(
                webview_id = %wv_id,
                ipc_id,
                error = %err,
                "hardware async worker failed"
            );
        } else {
            tracing::debug!(
                webview_id = %wv_id,
                ipc_id,
                "hardware async worker succeeded"
            );
        }
        if let Err(err) = proxy.send_event(UserEvent::HardwareSignResult {
            webview_id: wv_id,
            ipc_id,
            result,
        }) {
            tracing::warn!(
                error = %err,
                "failed to send HardwareSignResult from worker"
            );
        }
    });
}

fn with_connected_hardware_device<T, F>(
    hardware_signer: &std::sync::Arc<std::sync::Mutex<Option<crate::hardware::HardwareDevice>>>,
    task: F,
) -> std::result::Result<T, String>
where
    F: FnOnce(&crate::hardware::HardwareDevice) -> std::result::Result<T, String>,
{
    let hs = hardware_signer.lock().unwrap();
    let device = hs
        .as_ref()
        .ok_or_else(|| "Hardware wallet not connected".to_string())?;
    task(device)
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
