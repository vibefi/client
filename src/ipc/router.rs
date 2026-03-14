use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use wry::WebView;

use crate::ipc_contract::{IpcRequest, KnownProviderId};
use crate::registry::handle_launcher_ipc;
use crate::state::lock_or_err;
use crate::state::{AppState, PendingConnect, ProviderInfo, UserEvent, WalletBackend};
use crate::webview_manager::{AppWebViewKind, WebViewManager};

use super::{
    hardware, ipfs, local, respond_option_result, respond_value_result, selector, walletconnect,
};

fn resolve_code_state(state: &AppState) -> Result<AppState> {
    let code = lock_or_err(&state.code, "code")?;
    let Some(ctx) = code.anvil_context.as_ref() else {
        return Ok(state.clone());
    };

    let mut overlay = state.clone();
    overlay.signer = Arc::new(Mutex::new(Some(ctx.signer.clone())));
    overlay.wallet_backend = Arc::new(Mutex::new(Some(WalletBackend::Local)));
    overlay.wallet = Arc::clone(&ctx.wallet);
    overlay.walletconnect = Arc::new(Mutex::new(None));
    overlay.hardware_signer = Arc::new(Mutex::new(None));
    overlay.rpc_manager = Arc::new(Mutex::new(Some(ctx.rpc_manager.clone())));
    Ok(overlay)
}

pub fn handle_ipc(
    webview: &WebView,
    manager: &WebViewManager,
    state: &AppState,
    webview_id: &str,
    msg: String,
) -> Result<()> {
    let req: IpcRequest = serde_json::from_str(&msg).context("invalid IPC JSON")?;
    let app_kind = manager.app_kind_for_id(webview_id);
    let is_code_surface = app_kind == Some(AppWebViewKind::Code);
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
        let result =
            selector::handle_wallet_selector_ipc(webview, manager, state, webview_id, &req);
        respond_option_result(webview, req.id, result)?;
        return Ok(());
    }

    if provider == Some(KnownProviderId::Settings) {
        let settings_write_method = matches!(
            req.method.as_str(),
            "vibefi_setEndpoints"
                | "vibefi_setIpfsSettings"
                | "vibefi_setMaxConcurrentRpc"
                | "vibefi_setRpcAndIpfsSettings"
                | "vibefi_saveSettings"
                | "vibefi_openLogDirectory"
        );
        if settings_write_method {
            if manager.app_kind_for_id(webview_id) != Some(AppWebViewKind::Settings) {
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
        let is_launcher_surface = matches!(
            manager.app_kind_for_id(webview_id),
            Some(AppWebViewKind::Launcher | AppWebViewKind::Studio)
        );
        if !is_launcher_surface {
            tracing::warn!(
                webview_id,
                method = %req.method,
                "launcher ipc request rejected for non-launcher/studio webview"
            );
            bail!("launcher IPC is only available to launcher/studio webviews");
        }
        let result = handle_launcher_ipc(state, webview_id, &req);
        respond_option_result(webview, req.id, result)?;
        return Ok(());
    }

    if provider == Some(KnownProviderId::Code) {
        if manager.app_kind_for_id(webview_id) != Some(AppWebViewKind::Code) {
            tracing::warn!(
                webview_id,
                method = %req.method,
                "code ipc request rejected for non-code webview"
            );
            bail!("code IPC is only available to code webviews");
        }
        let result = crate::code::router::handle_code_ipc(state, manager, webview_id, &req);
        respond_option_result(webview, req.id, result)?;
        return Ok(());
    }

    if provider == Some(KnownProviderId::Automation) && state.automation {
        if req.method == "automation_result" {
            crate::automation::handle_automation_ipc_result(&req.params);
        }
        return Ok(());
    }

    if provider == Some(KnownProviderId::Ipfs) {
        let state_clone = state.clone();
        let webview_id = webview_id.to_string();
        let ipc_id = req.id;
        let req_clone = req.clone();
        std::thread::spawn(move || {
            let result = ipfs::handle_ipfs_ipc(&state_clone, &webview_id, &req_clone)
                .map(|value| value.unwrap_or(serde_json::Value::Null))
                .map_err(|err| err.to_string());
            let _ = state_clone.proxy.send_event(UserEvent::RpcResult {
                webview_id,
                ipc_id,
                result,
            });
        });
        return Ok(());
    }

    let is_connect_request = matches!(
        req.method.as_str(),
        "eth_requestAccounts" | "wallet_requestPermissions"
    );

    // Code preview is isolated from the global wallet backend and always routes
    // through the local provider path when a local signer is available.
    let code_state = if is_code_surface {
        Some(resolve_code_state(state)?)
    } else {
        None
    };
    if let Some(code_state) = code_state.as_ref() {
        if is_connect_request && code_state.local_signer().is_none() {
            return respond_option_result(
                webview,
                req.id,
                Err(anyhow!(
                    "Code preview only supports the local Anvil wallet. Start Anvil in the Code sidebar and try again."
                )),
            );
        }
        return respond_option_result(
            webview,
            req.id,
            local::handle_local_ipc(webview, code_state, webview_id, &req),
        );
    }

    let backend = state.get_wallet_backend();

    // If no wallet backend is chosen yet and the dapp calls eth_requestAccounts,
    // open the wallet selector tab and park the request (non-Code surfaces only).
    if backend.is_none() && req.method == "eth_requestAccounts" {
        {
            let mut pending = lock_or_err(&state.pending_connect, "pending_connect")?;
            pending.push_back(PendingConnect {
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
