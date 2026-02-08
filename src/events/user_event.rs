use tao::{event_loop::EventLoopProxy, window::Window};

use crate::ipc;
use crate::ipc_contract::{IpcRequest, KnownProviderId, TabbarMethod};
use crate::state::{AppState, TabAction, UserEvent};
use crate::ui_bridge;
use crate::webview::{EmbeddedContent, build_app_webview};
use crate::webview_manager::{AppWebViewEntry, WebViewManager};

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
                            manager.close_app(idx as usize);
                        }
                    }
                    None => {}
                }
            }
        }
    } else if let Some(wv) = manager.webview_for_id(webview_id) {
        if let Err(e) = ipc::handle_ipc(wv, state, webview_id, msg) {
            eprintln!("ipc error: {e:?}");
        }
    }
}

pub fn handle_open_wallet_selector(
    window: Option<&Window>,
    state: &AppState,
    manager: &mut WebViewManager,
    proxy: &EventLoopProxy<UserEvent>,
) {
    // Only open one selector at a time.
    {
        let sel = state.selector_webview_id.lock().unwrap();
        if sel.is_some() {
            // Already open — just switch to it
            if let Some(idx) = manager.index_of_label("Connect Wallet") {
                manager.switch_to(idx);
            }
            return;
        }
    }
    if let Some(w) = window {
        let size = w.inner_size();
        let id = manager.next_app_id();
        let bounds = manager.app_rect(size.width, size.height);
        match build_app_webview(
            w,
            &id,
            None,
            EmbeddedContent::WalletSelector,
            state,
            proxy.clone(),
            bounds,
        ) {
            Ok(wv) => {
                // Hide currently active before adding new
                if let Some(active) = manager.active_app_webview() {
                    let _ = active.set_visible(false);
                }
                let idx = manager.apps.len();
                {
                    let mut sel = state.selector_webview_id.lock().unwrap();
                    *sel = Some(id.clone());
                }
                manager.apps.push(AppWebViewEntry {
                    webview: wv,
                    id,
                    label: "Connect Wallet".to_string(),
                    dist_dir: None,
                });
                manager.active_app_index = Some(idx);
                manager.update_tab_bar();
            }
            Err(e) => eprintln!("failed to open wallet selector tab: {e:?}"),
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
        let pending = state.pending_connect.lock().unwrap().take();
        if let Some(pc) = pending {
            if pc.webview_id != webview_id {
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
}

pub fn handle_hardware_sign_result(
    manager: &WebViewManager,
    webview_id: String,
    ipc_id: u64,
    result: Result<String, String>,
) {
    if let Some(wv) = manager.webview_for_id(&webview_id) {
        match result {
            Ok(value) => {
                let json_val: serde_json::Value = serde_json::Value::String(value);
                if let Err(e) = ipc::respond_ok(wv, ipc_id, json_val) {
                    eprintln!("[hardware] failed to send ok response: {e}");
                }
            }
            Err(msg) => {
                if let Err(e) = ipc::respond_err(wv, ipc_id, &msg) {
                    eprintln!("[hardware] failed to send error response: {e}");
                }
            }
        }
    }
}

pub fn handle_open_settings(
    window: Option<&Window>,
    state: &AppState,
    manager: &mut WebViewManager,
    proxy: &EventLoopProxy<UserEvent>,
) {
    // Only open one settings tab at a time.
    {
        let sel = state.settings_webview_id.lock().unwrap();
        if sel.is_some() {
            if let Some(idx) = manager.index_of_label("Settings") {
                manager.switch_to(idx);
            }
            return;
        }
    }
    if let Some(w) = window {
        let size = w.inner_size();
        let id = manager.next_app_id();
        let bounds = manager.app_rect(size.width, size.height);
        match build_app_webview(
            w,
            &id,
            None,
            EmbeddedContent::Settings,
            state,
            proxy.clone(),
            bounds,
        ) {
            Ok(wv) => {
                if let Some(active) = manager.active_app_webview() {
                    let _ = active.set_visible(false);
                }
                let idx = manager.apps.len();
                {
                    let mut sel = state.settings_webview_id.lock().unwrap();
                    *sel = Some(id.clone());
                }
                manager.apps.push(AppWebViewEntry {
                    webview: wv,
                    id,
                    label: "Settings".to_string(),
                    dist_dir: None,
                });
                manager.active_app_index = Some(idx);
                manager.update_tab_bar();
            }
            Err(e) => eprintln!("failed to open settings tab: {e:?}"),
        }
    }
}

pub fn handle_close_wallet_selector(state: &AppState, manager: &mut WebViewManager) {
    {
        let mut sel = state.selector_webview_id.lock().unwrap();
        *sel = None;
    }
    manager.close_by_label("Connect Wallet");
}

pub fn handle_tab_action(
    window: Option<&Window>,
    state: &AppState,
    manager: &mut WebViewManager,
    proxy: &EventLoopProxy<UserEvent>,
    action: TabAction,
) {
    match action {
        TabAction::SwitchTab(i) => manager.switch_to(i),
        TabAction::CloseTab(i) => manager.close_app(i),
        TabAction::OpenApp { name, dist_dir } => {
            if let Some(w) = window {
                let size = w.inner_size();
                let id = manager.next_app_id();
                let bounds = manager.app_rect(size.width, size.height);
                match build_app_webview(
                    w,
                    &id,
                    Some(dist_dir.clone()),
                    EmbeddedContent::Default,
                    state,
                    proxy.clone(),
                    bounds,
                ) {
                    Ok(wv) => {
                        // Hide currently active before adding new
                        if let Some(active) = manager.active_app_webview() {
                            let _ = active.set_visible(false);
                        }
                        let idx = manager.apps.len();
                        manager.apps.push(AppWebViewEntry {
                            webview: wv,
                            id,
                            label: name,
                            dist_dir: Some(dist_dir),
                        });
                        manager.active_app_index = Some(idx);
                        manager.update_tab_bar();
                    }
                    Err(e) => eprintln!("failed to open app tab: {e:?}"),
                }
            }
        }
    }
}
