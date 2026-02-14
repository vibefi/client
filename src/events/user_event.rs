use std::path::PathBuf;

use tao::event_loop::EventLoopProxy;

use crate::ipc;
use crate::ipc_contract::{IpcRequest, KnownProviderId, TabbarMethod};
use crate::state::{AppState, TabAction, UserEvent};
use crate::ui_bridge;
use crate::webview::{EmbeddedContent, WebViewHost, build_app_webview};
use crate::webview_manager::{AppWebViewEntry, AppWebViewKind, WebViewManager};

pub fn handle_ipc_event(
    state: &AppState,
    manager: &mut WebViewManager,
    webview_id: &str,
    msg: String,
) {
    if webview_id == "tab-bar" {
        // Parse tab bar IPC
        if let Ok(req) = serde_json::from_str::<IpcRequest>(&msg) {
            if req.provider() == Some(KnownProviderId::Tabbar) {
                match req.tabbar_method() {
                    Some(TabbarMethod::SwitchTab) => {
                        if let Some(idx) = req.params.get(0).and_then(|v| v.as_u64()) {
                            manager.switch_to(idx as usize);
                        }
                    }
                    Some(TabbarMethod::CloseTab) => {
                        if let Some(idx) = req.params.get(0).and_then(|v| v.as_u64()) {
                            let idx = idx as usize;
                            if let Some(entry) = manager.apps.get(idx) {
                                if entry.kind == AppWebViewKind::Settings {
                                    let mut sel = state.settings_webview_id.lock().unwrap();
                                    *sel = None;
                                } else if entry.kind == AppWebViewKind::WalletSelector {
                                    let mut sel = state.selector_webview_id.lock().unwrap();
                                    *sel = None;
                                }
                            }
                            manager.close_app(idx);
                        }
                    }
                    None => {}
                }
            }
        }
    } else if let Some(wv) = manager.webview_for_id(webview_id) {
        if let Err(e) = ipc::handle_ipc(wv, manager, state, webview_id, msg) {
            tracing::error!(error = ?e, webview_id, "ipc error");
        }
    }
}

pub fn handle_open_wallet_selector(
    host: Option<&WebViewHost>,
    state: &AppState,
    manager: &mut WebViewManager,
    proxy: &EventLoopProxy<UserEvent>,
) {
    // Only open one selector at a time.
    {
        let sel = state.selector_webview_id.lock().unwrap();
        if sel.is_some() {
            // Already open — just switch to it
            if let Some(idx) = manager.index_of_kind(AppWebViewKind::WalletSelector) {
                manager.switch_to(idx);
            }
            return;
        }
    }
    if let Some(host) = host {
        match open_app_tab(
            host,
            state,
            manager,
            proxy,
            None,
            EmbeddedContent::WalletSelector,
            AppWebViewKind::WalletSelector,
            "Connect Wallet".to_string(),
        ) {
            Ok(id) => {
                let mut sel = state.selector_webview_id.lock().unwrap();
                *sel = Some(id);
            }
            Err(e) => tracing::error!(error = ?e, "failed to open wallet selector tab"),
        }
    }
}

pub fn handle_walletconnect_pairing(
    state: &AppState,
    manager: &WebViewManager,
    uri: String,
    qr_svg: String,
) {
    // Send pairing data to the wallet selector tab (if open).
    let sel_id = state.selector_webview_id.lock().unwrap().clone();
    if let Some(sel_id) = sel_id {
        if let Some(wv) = manager.webview_for_id(&sel_id) {
            ui_bridge::emit_walletconnect_pairing(wv, &uri, &qr_svg);
        }
    }
}

pub fn handle_walletconnect_result(
    state: &AppState,
    manager: &mut WebViewManager,
    webview_id: String,
    ipc_id: u64,
    result: Result<crate::walletconnect::WalletConnectSession, String>,
) {
    // Try the specific webview first, fall back to active
    let wv = manager
        .webview_for_id(&webview_id)
        .or_else(|| manager.active_app_webview());
    if let Some(wv) = wv {
        ipc::handle_walletconnect_connect_result(wv, state, ipc_id, result.clone());
    }

    // If there is a pending eth_requestAccounts from a dapp,
    // resolve it now that the wallet is connected.
    if let Ok(ref session) = result {
        let pending: Vec<_> = {
            let mut guard = state.pending_connect.lock().unwrap();
            guard.drain(..).collect()
        };
        for pc in pending {
            if pc.webview_id == webview_id && pc.ipc_id == ipc_id {
                continue;
            }
            if let Some(dapp_wv) = manager.webview_for_id(&pc.webview_id) {
                let accounts: Vec<serde_json::Value> = session
                    .accounts
                    .iter()
                    .map(|a| serde_json::Value::String(a.clone()))
                    .collect();
                let _ = ipc::respond_ok(dapp_wv, pc.ipc_id, serde_json::Value::Array(accounts));
            }
        }
    }
}

pub fn handle_hardware_sign_result(
    manager: &WebViewManager,
    webview_id: String,
    ipc_id: u64,
    result: Result<String, String>,
) {
    if let Some(wv) = manager.webview_for_id(&webview_id) {
        let is_ok = result.is_ok();
        let mapped = result.map(serde_json::Value::String);
        if let Err(e) = ipc::respond_value_result(wv, ipc_id, mapped) {
            if is_ok {
                tracing::error!(error = %e, "hardware: failed to send ok response");
            } else {
                tracing::error!(error = %e, "hardware: failed to send error response");
            }
        }
    }
}

pub fn handle_open_settings(
    host: Option<&WebViewHost>,
    state: &AppState,
    manager: &mut WebViewManager,
    proxy: &EventLoopProxy<UserEvent>,
) {
    // Only open one settings tab at a time.
    {
        let mut sel = state.settings_webview_id.lock().unwrap();
        if sel.is_some() {
            if let Some(idx) = manager.index_of_kind(AppWebViewKind::Settings) {
                manager.switch_to(idx);
                return;
            }
            // Stale ID (tab was closed). Clear and continue to open a new tab.
            *sel = None;
        }
    }
    if let Some(host) = host {
        match open_app_tab(
            host,
            state,
            manager,
            proxy,
            None,
            EmbeddedContent::Settings,
            AppWebViewKind::Settings,
            "Settings".to_string(),
        ) {
            Ok(id) => {
                let mut sel = state.settings_webview_id.lock().unwrap();
                *sel = Some(id);
            }
            Err(e) => tracing::error!(error = ?e, "failed to open settings tab"),
        }
    }
}

pub fn handle_rpc_result(
    manager: &WebViewManager,
    webview_id: String,
    ipc_id: u64,
    result: Result<serde_json::Value, String>,
) {
    if let Some(wv) = manager.webview_for_id(&webview_id) {
        let is_ok = result.is_ok();
        if let Err(e) = ipc::respond_value_result(wv, ipc_id, result) {
            if is_ok {
                tracing::error!(error = %e, "rpc: failed to send ok response");
            } else {
                tracing::error!(error = %e, "rpc: failed to send error response");
            }
        }
    }
}

pub fn handle_provider_event(
    manager: &WebViewManager,
    webview_id: String,
    event: String,
    value: serde_json::Value,
) {
    if let Some(wv) = manager.webview_for_id(&webview_id) {
        ui_bridge::emit_provider_event(wv, &event, value);
    }
}

pub fn handle_close_wallet_selector(state: &AppState, manager: &mut WebViewManager) {
    {
        let mut sel = state.selector_webview_id.lock().unwrap();
        *sel = None;
    }
    manager.close_by_kind(AppWebViewKind::WalletSelector);
}

pub fn handle_tab_action(
    host: Option<&WebViewHost>,
    state: &AppState,
    manager: &mut WebViewManager,
    proxy: &EventLoopProxy<UserEvent>,
    action: TabAction,
) {
    match action {
        TabAction::OpenApp { name, dist_dir } => {
            if let Some(host) = host {
                if let Err(e) = open_app_tab(
                    host,
                    state,
                    manager,
                    proxy,
                    Some(dist_dir),
                    EmbeddedContent::Default,
                    AppWebViewKind::Standard,
                    name,
                ) {
                    tracing::error!(error = ?e, "failed to open app tab");
                }
            }
        }
    }
}

fn open_app_tab(
    host: &WebViewHost,
    state: &AppState,
    manager: &mut WebViewManager,
    proxy: &EventLoopProxy<UserEvent>,
    dist_dir: Option<PathBuf>,
    embedded: EmbeddedContent,
    kind: AppWebViewKind,
    label: String,
) -> anyhow::Result<String> {
    let size = host.window.inner_size();
    let id = manager.next_app_id();
    let bounds = manager.app_rect(size.width, size.height);
    let webview = build_app_webview(host, &id, dist_dir, embedded, state, proxy.clone(), bounds)?;

    if let Some(active) = manager.active_app_webview() {
        let _ = active.set_visible(false);
    }
    let idx = manager.apps.len();
    manager.apps.push(AppWebViewEntry {
        webview,
        id,
        label,
        kind,
    });
    manager.active_app_index = Some(idx);
    manager.update_tab_bar();

    Ok(manager.apps[idx].id.clone())
}
