use serde::Deserialize;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::{fs, path::Path};
use tao::event_loop::EventLoopProxy;

use crate::ipc;
use crate::ipc_contract::{IpcRequest, KnownProviderId, TabbarMethod};
use crate::state::lock_or_err;
use crate::state::{AppRuntimeCapabilities, AppState, IpfsCapabilityRule, TabAction, UserEvent};
use crate::ui_bridge;
use crate::webview::{EmbeddedContent, WebViewHost, build_app_webview};
use crate::webview_manager::{AppWebViewEntry, AppWebViewKind, WebViewManager};

fn lock_or_log<'a, T>(mutex: &'a Mutex<T>, name: &str) -> Option<MutexGuard<'a, T>> {
    match lock_or_err(mutex, name) {
        Ok(guard) => Some(guard),
        Err(err) => {
            tracing::error!(error = %err, "failed to acquire lock");
            None
        }
    }
}

#[derive(Debug, Deserialize)]
struct BundleManifest {
    #[serde(default)]
    capabilities: Option<BundleCapabilities>,
}

#[derive(Debug, Deserialize)]
struct BundleCapabilities {
    #[serde(default)]
    ipfs: Option<BundleIpfsCapabilities>,
}

#[derive(Debug, Deserialize)]
struct BundleIpfsCapabilities {
    #[serde(default)]
    allow: Vec<BundleIpfsAllowRule>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BundleIpfsAllowRule {
    #[serde(default)]
    cid: Option<String>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(rename = "as", default)]
    as_: Vec<String>,
    #[serde(default)]
    max_bytes: Option<usize>,
}

pub(crate) fn load_app_capabilities_from_dist(dist_dir: &Path) -> AppRuntimeCapabilities {
    let Some(bundle_root) = dist_dir.parent().and_then(|p| p.parent()) else {
        return AppRuntimeCapabilities::default();
    };
    let manifest_path = bundle_root.join("manifest.json");
    let raw = match fs::read_to_string(&manifest_path) {
        Ok(raw) => raw,
        Err(_) => return AppRuntimeCapabilities::default(),
    };
    let parsed: BundleManifest = match serde_json::from_str(&raw) {
        Ok(parsed) => parsed,
        Err(_) => return AppRuntimeCapabilities::default(),
    };

    let rules = parsed
        .capabilities
        .and_then(|caps| caps.ipfs)
        .map(|ipfs| ipfs.allow)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|rule| {
            if rule.paths.is_empty() || rule.as_.is_empty() {
                return None;
            }
            Some(IpfsCapabilityRule {
                cid: rule
                    .cid
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
                paths: rule
                    .paths
                    .into_iter()
                    .map(|p| p.trim_start_matches('/').to_string())
                    .filter(|p| !p.is_empty())
                    .collect(),
                as_kinds: rule.as_.into_iter().map(|k| k.to_lowercase()).collect(),
                max_bytes: rule.max_bytes,
            })
        })
        .collect();

    AppRuntimeCapabilities { ipfs_allow: rules }
}

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
                                if !entry.kind.is_closeable() {
                                    tracing::debug!(
                                        index = idx,
                                        kind = ?entry.kind,
                                        "ignoring close request for non-closeable tab"
                                    );
                                    return;
                                }
                                {
                                    if let Some(mut caps) =
                                        lock_or_log(&state.app_capabilities, "app_capabilities")
                                    {
                                        caps.remove(&entry.id);
                                    }
                                }
                                if entry.kind == AppWebViewKind::Settings {
                                    if let Some(mut sel) = lock_or_log(
                                        &state.settings_webview_id,
                                        "settings_webview_id",
                                    ) {
                                        *sel = None;
                                    }
                                } else if entry.kind == AppWebViewKind::WalletSelector {
                                    if let Some(mut sel) = lock_or_log(
                                        &state.selector_webview_id,
                                        "selector_webview_id",
                                    ) {
                                        *sel = None;
                                    }
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
        let Some(sel) = lock_or_log(&state.selector_webview_id, "selector_webview_id") else {
            return;
        };
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
                if let Some(mut sel) =
                    lock_or_log(&state.selector_webview_id, "selector_webview_id")
                {
                    *sel = Some(id);
                }
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
    let sel_id = lock_or_log(&state.selector_webview_id, "selector_webview_id")
        .map(|id| id.clone())
        .flatten();
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
        let pending: Vec<_> = match lock_or_log(&state.pending_connect, "pending_connect") {
            Some(mut guard) => guard.drain(..).collect(),
            None => Vec::new(),
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
        let Some(mut sel) = lock_or_log(&state.settings_webview_id, "settings_webview_id") else {
            return;
        };
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
                if let Some(mut sel) =
                    lock_or_log(&state.settings_webview_id, "settings_webview_id")
                {
                    *sel = Some(id);
                }
            }
            Err(e) => tracing::error!(error = ?e, "failed to open settings tab"),
        }
    }
}

pub fn handle_rpc_pending_changed(manager: &WebViewManager, webview_id: &str, count: u32) {
    if let Some(tb) = manager.tab_bar.as_ref() {
        if let Err(err) = ui_bridge::update_rpc_status(tb, webview_id, count) {
            tracing::warn!(error = %err, "failed to dispatch rpc status update");
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
    if let Some(mut sel) = lock_or_log(&state.selector_webview_id, "selector_webview_id") {
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

pub fn handle_studio_bundle_resolved(
    host: Option<&WebViewHost>,
    state: &AppState,
    manager: &mut WebViewManager,
    proxy: &EventLoopProxy<UserEvent>,
    placeholder_id: String,
    result: Result<PathBuf, String>,
) {
    let Some(index) = manager.index_of_id(&placeholder_id) else {
        return;
    };

    match result {
        Ok(dist_dir) => {
            let Some(host) = host else {
                return;
            };
            let size = host.window.inner_size();
            let bounds = manager.app_rect(size.width, size.height);
            let studio_webview_id = manager.next_app_id();
            match build_app_webview(
                host,
                &studio_webview_id,
                Some(dist_dir.clone()),
                EmbeddedContent::Default,
                state,
                proxy.clone(),
                bounds,
            ) {
                Ok(webview) => {
                    if let Err(err) = webview.set_visible(false) {
                        tracing::warn!(error = %err, "failed to hide loaded studio webview");
                    }
                    if let Some(mut caps) = lock_or_log(&state.app_capabilities, "app_capabilities")
                    {
                        let studio_caps = load_app_capabilities_from_dist(&dist_dir);
                        caps.remove(&placeholder_id);
                        caps.insert(studio_webview_id.clone(), studio_caps);
                    }
                    manager.apps[index] = AppWebViewEntry {
                        webview,
                        id: studio_webview_id.clone(),
                        label: "Studio".to_string(),
                        kind: AppWebViewKind::Studio,
                        selectable: true,
                        loading: false,
                    };
                    if state.automation {
                        crate::automation::emit_webview_created(
                            &studio_webview_id,
                            "Studio",
                            "Studio",
                        );
                    }
                }
                Err(err) => {
                    tracing::error!(error = ?err, "failed to build loaded studio webview");
                    if let Some(entry) = manager.apps.get_mut(index) {
                        entry.label = "Studio (unavailable)".to_string();
                        entry.selectable = false;
                        entry.loading = false;
                    }
                }
            }
        }
        Err(err) => {
            tracing::warn!(error = %err, "failed to prepare studio dapp bundle");
            if let Some(entry) = manager.apps.get_mut(index) {
                entry.label = "Studio (unavailable)".to_string();
                entry.selectable = false;
                entry.loading = false;
            }
        }
    }

    manager.update_tab_bar();
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
    let app_capabilities = dist_dir
        .as_deref()
        .map(load_app_capabilities_from_dist)
        .unwrap_or_default();
    let webview = build_app_webview(host, &id, dist_dir, embedded, state, proxy.clone(), bounds)?;

    if let Some(active) = manager.active_app_webview() {
        let _ = active.set_visible(false);
    }
    let idx = manager.apps.len();
    if let Some(mut caps) = lock_or_log(&state.app_capabilities, "app_capabilities") {
        caps.insert(id.clone(), app_capabilities);
    }
    manager.apps.push(AppWebViewEntry {
        webview,
        id,
        label,
        kind,
        selectable: true,
        loading: false,
    });
    manager.active_app_index = Some(idx);
    manager.update_tab_bar();

    let entry = &manager.apps[idx];
    if state.automation {
        crate::automation::emit_webview_created(
            &entry.id,
            &format!("{:?}", entry.kind),
            &entry.label,
        );
    }

    Ok(entry.id.clone())
}
