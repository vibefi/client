use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;
use wry::WebView;

use crate::ipc_contract::{IpcRequest, KnownProviderId};
use crate::registry::handle_launcher_ipc;
use crate::state::{AppState, PendingConnect, ProviderInfo, UserEvent, WalletBackend};

use super::{hardware, local, respond_err, respond_ok, rpc, selector, walletconnect};

pub fn handle_ipc(
    webview: &WebView,
    state: &AppState,
    webview_id: &str,
    msg: String,
) -> Result<()> {
    let req: IpcRequest = serde_json::from_str(&msg).context("invalid IPC JSON")?;

    // Handle vibefi-wallet IPC from the wallet selector tab.
    if req.provider() == Some(KnownProviderId::Wallet) {
        let result = selector::handle_wallet_selector_ipc(webview, state, webview_id, &req);
        match result {
            Ok(Some(v)) => respond_ok(webview, req.id, v)?,
            Ok(None) => { /* response will be sent later */ }
            Err(e) => respond_err(webview, req.id, &e.to_string())?,
        }
        return Ok(());
    }

    if req.provider() == Some(KnownProviderId::Settings) {
        if req.method == "vibefi_setEndpoints" {
            let settings_id = state.settings_webview_id.lock().unwrap();
            if settings_id.as_deref() != Some(webview_id) {
                bail!("vibefi_setEndpoints is only available to the settings webview");
            }
        }
        let result = super::settings::handle_settings_ipc(state, &req);
        match result {
            Ok(v) => respond_ok(webview, req.id, v)?,
            Err(e) => respond_err(webview, req.id, &e.to_string())?,
        }
        return Ok(());
    }

    if req.provider() == Some(KnownProviderId::Launcher) {
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
        Some(WalletBackend::Local) => local::handle_local_ipc(webview, state, webview_id, &req),
        Some(WalletBackend::WalletConnect) => {
            walletconnect::handle_walletconnect_ipc(webview, state, webview_id, &req)
        }
        Some(WalletBackend::Hardware) => hardware::handle_hardware_ipc(state, webview_id, &req),
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
                    if state.network.is_some() && rpc::is_rpc_passthrough(req.method.as_str()) {
                        let proxy = state.proxy.clone();
                        let state_clone = state.clone();
                        let ipc_id = req.id;
                        let method = req.method.clone();
                        let params = req.params.clone();
                        let wv_id = webview_id.to_string();
                        std::thread::spawn(move || {
                            let req = crate::ipc_contract::IpcRequest {
                                id: ipc_id,
                                provider_id: None,
                                method,
                                params,
                            };
                            let result = rpc::proxy_rpc(&state_clone, &req)
                                .map_err(|e| e.to_string());
                            let _ = proxy.send_event(UserEvent::RpcResult {
                                webview_id: wv_id,
                                ipc_id,
                                result,
                            });
                        });
                        Ok(None)
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
