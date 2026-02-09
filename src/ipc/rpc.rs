use alloy_consensus::TypedTransaction;
use alloy_eips::eip2718::Encodable2718;
use alloy_primitives::{Address, Signature};
use alloy_rpc_types_eth::TransactionRequest;
use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

use crate::ipc_contract::IpcRequest;
use crate::state::AppState;

pub(super) fn is_rpc_passthrough(method: &str) -> bool {
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

pub(super) fn proxy_rpc(state: &AppState, req: &IpcRequest) -> Result<Value> {
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

    // Try RpcEndpointManager first (supports failover)
    let v = {
        let mut mgr = state.rpc_manager.lock().unwrap();
        if let Some(m) = mgr.as_mut() {
            m.send_rpc(&payload)?
        } else {
            // Fallback: use network.rpc_url directly
            let network = state.network.as_ref().ok_or_else(|| {
                anyhow!("No RPC endpoint configured. Provide a config file with rpcUrl.")
            })?;
            let res = network
                .http
                .post(&network.rpc_url)
                .json(&payload)
                .send()
                .context("rpc request failed")?;
            res.json().context("rpc decode failed")?
        }
    };

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
    if state.network.is_none() {
        bail!("No RPC endpoint configured. Provide a config file with rpcUrl.");
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

pub(super) fn build_filled_tx_request(
    state: &AppState,
    tx_obj: Value,
) -> Result<TransactionRequest> {
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

pub(super) fn build_typed_tx(mut tx: TransactionRequest) -> Result<TypedTransaction> {
    tx.trim_conflicting_keys();
    tx.build_typed_tx().map_err(|req| {
        let details = match req.missing_keys() {
            Ok(ty) => format!("transaction is not buildable for {:?}", ty),
            Err((ty, missing)) => format!("{:?} missing: {}", ty, missing.join(", ")),
        };
        anyhow!("unable to build signable transaction: {details}")
    })
}

pub(super) fn encode_signed_typed_tx_hex(tx: TypedTransaction, signature: Signature) -> String {
    let envelope = tx.into_envelope(signature);
    format!("0x{}", hex::encode(envelope.encoded_2718()))
}

pub(super) fn send_raw_transaction(state: &AppState, raw_tx_hex: String) -> Result<String> {
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

pub(super) fn parse_hex_u64(s: &str) -> Option<u64> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let s = if s.is_empty() { "0" } else { s };
    u64::from_str_radix(s, 16).ok()
}

pub(super) fn parse_hex_u128(s: &str) -> Option<u128> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let s = if s.is_empty() { "0" } else { s };
    u128::from_str_radix(s, 16).ok()
}

pub(super) fn decode_0x_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.strip_prefix("0x")?;
    if s.len() % 2 != 0 {
        return None;
    }
    hex::decode(s).ok()
}
