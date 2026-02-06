use alloy_primitives::B256;
use alloy_signer::SignerSync;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use wry::WebView;

use crate::devnet::handle_launcher_ipc;
use crate::state::{AppState, IpcRequest, ProviderInfo, WalletBackend};
use crate::walletconnect::HelperEvent;

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

    let result = match state.wallet_backend {
        WalletBackend::Local => handle_local_ipc(webview, state, &req),
        WalletBackend::WalletConnect => handle_walletconnect_ipc(webview, state, &req),
    };

    match result {
        Ok(v) => respond_ok(webview, req.id, v)?,
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
    webview: &WebView,
    state: &AppState,
    req: &IpcRequest,
) -> Result<Value> {
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
                .ok_or_else(|| anyhow!("walletconnect bridge unavailable"))?;
            let mut bridge = bridge.lock().unwrap();
            let session = bridge.connect_with_event_handler(chain_id, |event| {
                apply_walletconnect_event(webview, state, event);
            })?;
            drop(bridge);

            let accounts = session
                .accounts
                .iter()
                .map(|a| Value::String(a.clone()))
                .collect::<Vec<_>>();
            let chain_id = parse_hex_u64(&session.chain_id_hex).unwrap_or(chain_id);
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
            emit_chain_changed(webview, session.chain_id_hex);
            eprintln!(
                "[walletconnect] eth_requestAccounts resolved ({} account(s))",
                session.accounts.len()
            );
            Ok(Value::Array(accounts))
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
            Ok(value)
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
            Ok(value)
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
            Ok(Value::String(chain_id.to_string()))
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
            Ok(serde_json::to_value(info)?)
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
            Ok(value)
        }
        _ => walletconnect_request(webview, state, req.method.as_str(), req.params.clone()),
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
                println!("[WalletConnect] pairing uri: {uri}");
                {
                    let mut ws = state.wallet.lock().unwrap();
                    ws.walletconnect_uri = Some(uri.clone());
                }
                show_walletconnect_pairing_overlay(webview, &uri);
                let payload = serde_json::json!({
                    "type": "walletconnect_uri",
                    "data": uri
                });
                let js = format!("window.__WryEthereumEmit('message', {});", payload);
                if let Err(err) = webview.evaluate_script(&js) {
                    eprintln!("[walletconnect] failed to emit message event to webview: {err}");
                }
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
                hide_walletconnect_pairing_overlay(webview);
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

fn show_walletconnect_pairing_overlay(webview: &WebView, uri: &str) {
    let uri_json = match serde_json::to_string(uri) {
        Ok(v) => v,
        Err(err) => {
            eprintln!("[walletconnect] failed to serialize pairing uri for overlay: {err}");
            return;
        }
    };
    let js = format!(
        r#"(function() {{
  try {{
    var uri = {uri_json};
    var panel = document.getElementById('__vibefi_wc_overlay');
    if (!panel) {{
      panel = document.createElement('div');
      panel.id = '__vibefi_wc_overlay';
      panel.style.position = 'fixed';
      panel.style.right = '12px';
      panel.style.bottom = '12px';
      panel.style.width = 'min(560px, calc(100vw - 24px))';
      panel.style.background = 'rgba(2, 6, 23, 0.96)';
      panel.style.color = '#e2e8f0';
      panel.style.border = '1px solid rgba(148, 163, 184, 0.35)';
      panel.style.borderRadius = '12px';
      panel.style.padding = '12px';
      panel.style.fontSize = '12px';
      panel.style.lineHeight = '1.4';
      panel.style.zIndex = '2147483647';
      panel.style.boxShadow = '0 20px 40px rgba(0, 0, 0, 0.4)';
      panel.style.display = 'none';

      var header = document.createElement('div');
      header.style.display = 'flex';
      header.style.justifyContent = 'space-between';
      header.style.alignItems = 'center';
      header.style.gap = '8px';
      header.style.marginBottom = '8px';
      var title = document.createElement('strong');
      title.textContent = 'WalletConnect Pairing';
      var hideBtn = document.createElement('button');
      hideBtn.textContent = 'Hide';
      hideBtn.style.border = '1px solid #475569';
      hideBtn.style.background = '#0f172a';
      hideBtn.style.color = '#e2e8f0';
      hideBtn.style.borderRadius = '8px';
      hideBtn.style.padding = '4px 8px';
      hideBtn.style.cursor = 'pointer';
      hideBtn.addEventListener('click', function() {{ panel.style.display = 'none'; }});
      header.appendChild(title);
      header.appendChild(hideBtn);

      var description = document.createElement('div');
      description.style.opacity = '0.9';
      description.style.marginBottom = '8px';
      description.textContent = 'Open a WalletConnect-compatible wallet and approve the session. You can copy the pairing URI below.';

      var area = document.createElement('textarea');
      area.id = '__vibefi_wc_uri';
      area.readOnly = true;
      area.style.width = '100%';
      area.style.height = '92px';
      area.style.background = '#020617';
      area.style.color = '#93c5fd';
      area.style.border = '1px solid #1e293b';
      area.style.borderRadius = '8px';
      area.style.padding = '8px';
      area.style.resize = 'vertical';
      area.style.fontFamily = 'ui-monospace, Menlo, Monaco, Consolas, monospace';

      var footer = document.createElement('div');
      footer.style.display = 'flex';
      footer.style.justifyContent = 'flex-end';
      footer.style.marginTop = '8px';
      var copyBtn = document.createElement('button');
      copyBtn.textContent = 'Copy URI';
      copyBtn.style.border = '1px solid #475569';
      copyBtn.style.background = '#0f172a';
      copyBtn.style.color = '#e2e8f0';
      copyBtn.style.borderRadius = '8px';
      copyBtn.style.padding = '6px 10px';
      copyBtn.style.cursor = 'pointer';
      copyBtn.addEventListener('click', async function() {{
        var value = area.value || '';
        if (!value) return;
        try {{
          if (navigator.clipboard && navigator.clipboard.writeText) {{
            await navigator.clipboard.writeText(value);
            return;
          }}
        }} catch (_) {{}}
        try {{
          area.focus();
          area.select();
          document.execCommand('copy');
        }} catch (_) {{}}
      }});
      footer.appendChild(copyBtn);

      panel.appendChild(header);
      panel.appendChild(description);
      panel.appendChild(area);
      panel.appendChild(footer);
      document.documentElement.appendChild(panel);
    }}
    var uriArea = document.getElementById('__vibefi_wc_uri');
    if (uriArea) uriArea.value = uri;
    panel.style.display = 'block';
  }} catch (err) {{
    console.error('vibefi wc overlay error', err);
  }}
}})();"#,
        uri_json = uri_json
    );
    if let Err(err) = webview.evaluate_script(&js) {
        eprintln!("[walletconnect] failed to show pairing overlay: {err}");
    }
}

fn hide_walletconnect_pairing_overlay(webview: &WebView) {
    let js = r#"(function() {
  var panel = document.getElementById('__vibefi_wc_overlay');
  if (panel) panel.style.display = 'none';
})();"#;
    if let Err(err) = webview.evaluate_script(js) {
        eprintln!("[walletconnect] failed to hide pairing overlay: {err}");
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
