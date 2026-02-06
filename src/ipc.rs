use alloy_primitives::{Address, B256};
use alloy_signer::SignerSync;
use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use wry::WebView;

use crate::devnet::handle_launcher_ipc;
use crate::state::{AppState, IpcRequest, ProviderInfo};

pub fn handle_ipc(webview: &WebView, state: &AppState, msg: String) -> Result<()> {
    let req: IpcRequest = serde_json::from_str(&msg).context("invalid IPC JSON")?;
    if matches!(req.provider_id.as_deref(), Some("vibefi-launcher")) {
        let result = handle_launcher_ipc(webview, state, &req);
        match result {
            Ok(v) => respond_ok(webview, req.id, v)?,
            Err(e) => respond_err(webview, req.id, &e.to_string())?,
        }
        return Ok(());
    }

    // Dispatch EIP-1193 methods.
    let result = match req.method.as_str() {
        // --- Basic identity ---
        "eth_chainId" => Ok(Value::String(state.chain_id_hex())),
        "net_version" => {
            let chain_id = state.wallet.lock().unwrap().chain.chain_id;
            Ok(Value::String(chain_id.to_string()))
        }

        // --- Accounts ---
        "eth_accounts" => {
            let ws = state.wallet.lock().unwrap();
            if ws.authorized {
                Ok(Value::Array(vec![Value::String(format!("0x{:x}", state.address()))]))
            } else {
                Ok(Value::Array(vec![]))
            }
        }
        "eth_requestAccounts" => {
            let mut ws = state.wallet.lock().unwrap();
            ws.authorized = true;
            drop(ws);

            let addr = state.address();
            emit_accounts_changed(webview, vec![addr]);

            Ok(Value::Array(vec![Value::String(format!("0x{:x}", addr))]))
        }

        // --- Chain switching (demo supports a small allowlist) ---
        "wallet_switchEthereumChain" => {
            // params: [{ chainId: "0x..." }]
            let chain_id_hex = req
                .params
                .get(0)
                .and_then(|v| v.get("chainId"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("invalid params for wallet_switchEthereumChain"))?;

            let chain_id = parse_hex_u64(chain_id_hex)
                .ok_or_else(|| anyhow!("invalid chainId"))?;

            // Demo allowlist: mainnet (1), sepolia (11155111), anvil (31337)
            if !matches!(chain_id, 1 | 11155111 | 31337) {
                return Err(anyhow!("Unsupported chainId in demo"));
            }

            {
                let mut ws = state.wallet.lock().unwrap();
                ws.chain.chain_id = chain_id;
            }

            let chain_hex = format!("0x{:x}", chain_id);
            emit_chain_changed(webview, chain_hex.clone());

            // EIP-1193: success -> null
            Ok(Value::Null)
        }

        // --- Signing (offline) ---
        // personal_sign: params [message, address]
        "personal_sign" => {
            let msg = req
                .params
                .get(0)
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("invalid params for personal_sign"))?;

            // Accept either raw string or 0x hex data.
            let bytes = if let Some(b) = decode_0x_hex(msg) {
                b
            } else {
                msg.as_bytes().to_vec()
            };

            let sig = state
                .signer
                .sign_message_sync(&bytes)
                .context("sign_message failed")?;

            Ok(Value::String(format!("0x{}", hex::encode(sig.as_bytes()))))
        }

        // eth_signTypedData_v4 (demo): params [address, jsonString]
        "eth_signTypedData_v4" => {
            let typed_data_json = req
                .params
                .get(1)
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("invalid params for eth_signTypedData_v4"))?;

            let hash = alloy_primitives::keccak256(typed_data_json.as_bytes());
            let sig = state
                .signer
                .sign_hash_sync(&B256::from(hash))
                .context("sign_hash failed")?;

            Ok(Value::String(format!("0x{}", hex::encode(sig.as_bytes()))))
        }

        // eth_sendTransaction: params [txObject]
        "eth_sendTransaction" => {
            let ws = state.wallet.lock().unwrap();
            if !ws.authorized {
                return Err(anyhow!("Unauthorized: call eth_requestAccounts first"));
            }
            drop(ws);

            // If devnet is configured, proxy to anvil for real transaction execution
            if state.devnet.is_some() {
                // Ensure from address is set to our wallet address
                let mut tx_obj = req
                    .params
                    .get(0)
                    .cloned()
                    .ok_or_else(|| anyhow!("invalid params for eth_sendTransaction"))?;

                // Set from address if not present
                if tx_obj.get("from").is_none() {
                    if let Some(obj) = tx_obj.as_object_mut() {
                        obj.insert("from".to_string(), Value::String(format!("0x{:x}", state.address())));
                    }
                }

                // Create modified request with updated params
                let modified_req = IpcRequest {
                    id: req.id,
                    provider_id: req.provider_id.clone(),
                    method: req.method.clone(),
                    params: Value::Array(vec![tx_obj]),
                };

                proxy_rpc(state, &modified_req)
            } else {
                // Fallback: demo mode - hash the tx and return fake hash (no network)
                let tx_obj = req
                    .params
                    .get(0)
                    .cloned()
                    .ok_or_else(|| anyhow!("invalid params for eth_sendTransaction"))?;

                let canonical = serde_json::to_vec(&tx_obj).context("tx json encode")?;
                let digest = alloy_primitives::keccak256(&canonical);
                let sig = state
                    .signer
                    .sign_hash_sync(&B256::from(digest))
                    .context("sign_hash failed")?;

                let tx_hash = alloy_primitives::keccak256(sig.as_bytes());
                Ok(Value::String(format!("0x{}", hex::encode(tx_hash))))
            }
        }

        // EIP-1193 provider info (non-standard but useful)
        "wallet_getProviderInfo" => {
            let info = ProviderInfo {
                name: "wry-demo-wallet",
                chain_id: state.chain_id_hex(),
            };
            Ok(serde_json::to_value(info)?)
        }

        _ => {
            if state.devnet.is_some() && is_rpc_passthrough(req.method.as_str()) {
                proxy_rpc(state, &req)
            } else {
                Err(anyhow!("Unsupported method: {}", req.method))
            }
        }
    };

    match result {
        Ok(v) => respond_ok(webview, req.id, v)?,
        Err(e) => respond_err(webview, req.id, &e.to_string())?,
    }

    Ok(())
}

pub fn respond_ok(webview: &WebView, id: u64, value: Value) -> Result<()> {
    let js = format!(
        "window.__WryEthereumResolve({}, {}, null);",
        id,
        value.to_string()
    );
    webview.evaluate_script(&js)?;
    Ok(())
}

pub fn respond_err(webview: &WebView, id: u64, message: &str) -> Result<()> {
    // EIP-1193 style error object
    let err = serde_json::json!({
        "code": -32601,
        "message": message,
    });
    let js = format!(
        "window.__WryEthereumResolve({}, null, {});",
        id,
        err.to_string()
    );
    webview.evaluate_script(&js)?;
    Ok(())
}

pub fn emit_accounts_changed(webview: &WebView, addrs: Vec<Address>) {
    let arr = addrs
        .into_iter()
        .map(|a| Value::String(format!("0x{:x}", a)))
        .collect::<Vec<_>>();
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
    let devnet = state.devnet.as_ref().ok_or_else(|| anyhow!("Devnet not configured"))?;
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": req.method,
        "params": req.params,
    });

    // Log RPC request
    println!("[RPC] -> {} params={}", req.method, serde_json::to_string(&req.params).unwrap_or_default());

    let res = devnet
        .http
        .post(&devnet.rpc_url)
        .json(&payload)
        .send()
        .context("rpc request failed")?;
    let v: Value = res.json().context("rpc decode failed")?;

    // Log RPC response (truncate if too long)
    let result_str = v.get("result").map(|r| {
        let s = r.to_string();
        if s.len() > 200 { format!("{}...", &s[..200]) } else { s }
    }).unwrap_or_else(|| "null".to_string());

    if let Some(err) = v.get("error") {
        println!("[RPC] <- {} ERROR: {}", req.method, err);
        return Err(anyhow!("rpc error: {}", err));
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
