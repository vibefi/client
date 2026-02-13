use alloy_network::TxSignerSync;
use alloy_primitives::{B256, Signature};
use alloy_signer::SignerSync;
use anyhow::{Result, anyhow};
use serde_json::Value;
use wry::WebView;

use crate::ipc_contract::IpcRequest;
use crate::state::{AppState, ProviderInfo, UserEvent};

use super::rpc::{
    build_filled_tx_request, build_typed_tx, decode_0x_hex, encode_signed_typed_tx_hex,
    parse_hex_u64, send_raw_transaction,
};
use super::{emit_accounts_changed, emit_chain_changed, try_spawn_rpc_passthrough};

pub(super) fn handle_local_ipc(
    webview: &WebView,
    state: &AppState,
    webview_id: &str,
    req: &IpcRequest,
) -> Result<Option<Value>> {
    if let Some(value) = super::network_identity_response(state, req.method.as_str()) {
        return Ok(Some(value));
    }

    match req.method.as_str() {
        "eth_accounts" => {
            let ws = state.wallet.lock().unwrap();
            if ws.authorized {
                if let Some(account) = ws.account.clone().or_else(|| state.local_signer_address()) {
                    Ok(Some(Value::Array(vec![Value::String(account)])))
                } else {
                    Ok(Some(Value::Array(vec![])))
                }
            } else {
                Ok(Some(Value::Array(vec![])))
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
            Ok(Some(Value::Array(vec![Value::String(account)])))
        }
        "wallet_switchEthereumChain" => {
            let chain_id_hex = req
                .params
                .get(0)
                .and_then(|v| v.get("chainId"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("invalid params for wallet_switchEthereumChain"))?;
            let chain_id = parse_hex_u64(chain_id_hex).ok_or_else(|| anyhow!("invalid chainId"))?;

            {
                let mut ws = state.wallet.lock().unwrap();
                ws.chain.chain_id = chain_id;
            }
            let chain_hex = format!("0x{:x}", chain_id);
            emit_chain_changed(webview, chain_hex);
            Ok(Some(Value::Null))
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
                .map_err(|e| anyhow!("sign_message failed: {e}"))?;
            Ok(Some(Value::String(format!(
                "0x{}",
                hex::encode(sig.as_bytes())
            ))))
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
                .map_err(|e| anyhow!("sign_hash failed: {e}"))?;
            Ok(Some(Value::String(format!(
                "0x{}",
                hex::encode(sig.as_bytes())
            ))))
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

            let proxy = state.proxy.clone();
            let state_clone = state.clone();
            let ipc_id = req.id;
            let wv_id = webview_id.to_string();

            std::thread::spawn(move || {
                let result = (|| -> Result<Value> {
                    let tx_request = build_filled_tx_request(&state_clone, tx_obj)?;
                    let mut tx = build_typed_tx(tx_request)?;
                    let signer = state_clone
                        .local_signer()
                        .ok_or_else(|| anyhow!("Local signer unavailable"))?;
                    let sig: Signature = signer
                        .sign_transaction_sync(&mut tx)
                        .map_err(|e| anyhow!("sign_transaction failed: {e}"))?;
                    let raw_tx_hex = encode_signed_typed_tx_hex(tx, sig);
                    let tx_hash = send_raw_transaction(&state_clone, raw_tx_hex)?;
                    Ok(Value::String(tx_hash))
                })()
                .map_err(|e| e.to_string());
                let _ = proxy.send_event(UserEvent::RpcResult {
                    webview_id: wv_id,
                    ipc_id,
                    result,
                });
            });

            Ok(None)
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
            Ok(Some(serde_json::to_value(info)?))
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
