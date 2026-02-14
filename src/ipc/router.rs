use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;
use wry::WebView;

use crate::ipc_contract::{IpcRequest, KnownProviderId};
use crate::registry::handle_launcher_ipc;
use crate::state::{AppState, PendingConnect, ProviderInfo, UserEvent, WalletBackend};

use super::{
    hardware, local, respond_option_result, respond_value_result, selector, walletconnect,
};

pub fn handle_ipc(
    webview: &WebView,
    state: &AppState,
    webview_id: &str,
    msg: String,
) -> Result<()> {
    let req: IpcRequest = serde_json::from_str(&msg).context("invalid IPC JSON")?;
    let provider = req.provider();
    tracing::debug!(
        webview_id,
        provider = ?provider,
        method = %req.method,
        ipc_id = req.id,
        "ipc request received"
    );

    // Handle vibefi-wallet IPC from the wallet selector tab.
    if provider == Some(KnownProviderId::Wallet) {
        let result = selector::handle_wallet_selector_ipc(webview, state, webview_id, &req);
        respond_option_result(webview, req.id, result)?;
        return Ok(());
    }

    if provider == Some(KnownProviderId::Settings) {
        if req.method == "vibefi_setEndpoints" || req.method == "vibefi_setIpfsSettings" {
            let settings_id = state.settings_webview_id.lock().unwrap();
            if settings_id.as_deref() != Some(webview_id) {
                tracing::warn!(
                    webview_id,
                    method = %req.method,
                    "settings write attempt from non-settings webview"
                );
                bail!("settings write methods are only available to the settings webview");
            }
        }
        let result = super::settings::handle_settings_ipc(state, &req).map_err(|e| e.to_string());
        respond_value_result(webview, req.id, result)?;
        return Ok(());
    }

    if provider == Some(KnownProviderId::Launcher) {
        if webview_id != "app-0" {
            tracing::warn!(
                webview_id,
                method = %req.method,
                "launcher ipc request rejected for non-launcher webview"
            );
            bail!("launcher IPC is only available to the launcher webview");
        }
        let result = handle_launcher_ipc(state, webview_id, &req);
        respond_option_result(webview, req.id, result)?;
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
        tracing::info!(
            webview_id,
            ipc_id = req.id,
            "queued pending eth_requestAccounts and opening wallet selector"
        );
        if let Err(err) = state.proxy.send_event(UserEvent::OpenWalletSelector) {
            tracing::warn!(error = %err, "failed to send OpenWalletSelector event");
        }
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
            if let Some(value) = super::network_identity_response(state, req.method.as_str()) {
                return respond_option_result(webview, req.id, Ok(Some(value)));
            }

            // For methods other than eth_requestAccounts when no wallet is selected,
            // return sensible defaults.
            match req.method.as_str() {
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
                    if super::try_spawn_rpc_passthrough(state, webview_id, &req) {
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

    respond_option_result(webview, req.id, result)?;

    Ok(())
}
