mod bundle;
mod devnet;
mod hardware;
mod ipc;
mod menu;
mod state;
mod ui_bridge;
mod walletconnect;
mod webview;
mod webview_manager;

use anyhow::{Context, Result, anyhow};
use std::{
    env,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tao::{
    dpi::LogicalSize,
    event::{Event, StartCause, WindowEvent},
    event_loop::ControlFlow,
    window::WindowBuilder,
};

use reqwest::blocking::Client as HttpClient;

use bundle::{BundleConfig, build_bundle, verify_manifest};
use devnet::{DevnetConfig, DevnetContext, load_devnet};
use ipc::{handle_ipc, handle_walletconnect_connect_result};
use state::{AppState, Chain, LauncherConfig, TabAction, UserEvent, WalletState};
use webview::{EmbeddedContent, build_app_webview, build_tab_bar_webview};
use webview_manager::{AppWebViewEntry, WebViewManager};

static INDEX_HTML: &str = include_str!("../internal-ui/static/home.html");
static LAUNCHER_HTML: &str = include_str!("../internal-ui/static/launcher.html");
static TAB_BAR_HTML: &str = include_str!("../internal-ui/static/tabbar.html");
static WALLET_SELECTOR_HTML: &str = include_str!("../internal-ui/static/wallet-selector.html");
static HOME_JS: &str = include_str!("../internal-ui/dist/home.js");
static LAUNCHER_JS: &str = include_str!("../internal-ui/dist/launcher.js");
static TAB_BAR_JS: &str = include_str!("../internal-ui/dist/tabbar.js");
static WALLET_SELECTOR_JS: &str = include_str!("../internal-ui/dist/wallet-selector.js");
static PRELOAD_APP_JS: &str = include_str!("../internal-ui/dist/preload-app.js");
static PRELOAD_WALLET_SELECTOR_JS: &str =
    include_str!("../internal-ui/dist/preload-wallet-selector.js");
static PRELOAD_TAB_BAR_JS: &str = include_str!("../internal-ui/dist/preload-tabbar.js");

/// Hard-coded demo private key (DO NOT USE IN PRODUCTION).
/// This matches a common dev key used across many tutorials.
pub(crate) static DEMO_PRIVKEY_HEX: &str =
    "0x59c6995e998f97a5a0044966f094538c5f0f7b4b5b5b5b5b5b5b5b5b5b5b5b5b";

fn main() -> Result<()> {
    let args = parse_args()?;
    let bundle = args.bundle;
    let launcher = args.launcher;

    let devnet = launcher
        .as_ref()
        .and_then(|cfg| cfg.devnet_path.as_ref())
        .and_then(|path| load_devnet(path).ok());

    let initial_chain_id = devnet
        .as_ref()
        .map(|cfg| cfg.chainId)
        .unwrap_or_else(|| if launcher.is_some() { 31337 } else { 1 });

    // --- Window + event loop ---
    let mut event_loop = tao::event_loop::EventLoopBuilder::<UserEvent>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};

        event_loop.set_activation_policy(ActivationPolicy::Regular);
        event_loop.set_dock_visibility(true);
        event_loop.set_activate_ignoring_other_apps(true);
        menu::setup_macos_app_menu("Wry EIP-1193 demo");
    }
    let proxy = event_loop.create_proxy();

    let state = AppState {
        wallet: Arc::new(Mutex::new(WalletState {
            authorized: false,
            chain: Chain {
                chain_id: initial_chain_id,
            },
            account: None,
            walletconnect_uri: None,
        })),
        wallet_backend: Arc::new(Mutex::new(None)),
        signer: Arc::new(Mutex::new(None)),
        walletconnect: Arc::new(Mutex::new(None)),
        hardware_signer: Arc::new(Mutex::new(None)),
        devnet: launcher.as_ref().map(|cfg| DevnetContext {
            config: devnet.clone().unwrap_or(DevnetConfig {
                chainId: 31337,
                deployBlock: None,
                dappRegistry: String::new(),
                developerPrivateKey: None,
            }),
            rpc_url: cfg.rpc_url.clone(),
            ipfs_api: cfg.ipfs_api.clone(),
            ipfs_gateway: cfg.ipfs_gateway.clone(),
            cache_dir: cfg.cache_dir.clone(),
            http: HttpClient::new(),
        }),
        proxy: proxy.clone(),
        pending_connect: Arc::new(Mutex::new(None)),
        wc_project_id: args.wc_project_id,
        wc_relay_url: args.wc_relay_url,
        selector_webview_id: Arc::new(Mutex::new(None)),
    };
    let mut manager = WebViewManager::new(1.0);
    let mut window: Option<tao::window::Window> = None;

    event_loop.run(move |event, event_loop_window_target, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::Ipc { webview_id, msg }) => {
                if webview_id == "tab-bar" {
                    // Parse tab bar IPC
                    if let Ok(req) = serde_json::from_str::<state::IpcRequest>(&msg) {
                        if req.provider_id.as_deref() == Some("vibefi-tabbar") {
                            match req.method.as_str() {
                                "switchTab" => {
                                    if let Some(idx) = req.params.get(0).and_then(|v| v.as_u64()) {
                                        manager.switch_to(idx as usize);
                                    }
                                }
                                "closeTab" => {
                                    if let Some(idx) = req.params.get(0).and_then(|v| v.as_u64()) {
                                        manager.close_app(idx as usize);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                } else if let Some(wv) = manager.webview_for_id(&webview_id) {
                    if let Err(e) = handle_ipc(wv, &state, &webview_id, msg) {
                        eprintln!("ipc error: {e:?}");
                    }
                }
            }
            Event::UserEvent(UserEvent::OpenWalletSelector) => {
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
                if let Some(w) = window.as_ref() {
                    let size = w.inner_size();
                    let id = manager.next_app_id();
                    let bounds = manager.app_rect(size.width, size.height);
                    match build_app_webview(
                        w,
                        &id,
                        None,
                        EmbeddedContent::WalletSelector,
                        &state,
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
            Event::UserEvent(UserEvent::WalletConnectPairing { uri, qr_svg }) => {
                // Send pairing data to the wallet selector tab (if open).
                let sel_id = state.selector_webview_id.lock().unwrap().clone();
                if let Some(sel_id) = sel_id {
                    if let Some(wv) = manager.webview_for_id(&sel_id) {
                        ui_bridge::emit_walletconnect_pairing(wv, &uri, &qr_svg);
                    }
                }
            }
            Event::UserEvent(UserEvent::WalletConnectResult {
                webview_id,
                ipc_id,
                result,
            }) => {
                // Try the specific webview first, fall back to active
                let wv = manager
                    .webview_for_id(&webview_id)
                    .or_else(|| manager.active_app_webview());
                if let Some(wv) = wv {
                    handle_walletconnect_connect_result(wv, &state, ipc_id, result.clone());
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
                                let _ = ipc::respond_ok(
                                    dapp_wv,
                                    pc.ipc_id,
                                    serde_json::Value::Array(accounts),
                                );
                            }
                        }
                    }
                }
            }
            Event::UserEvent(UserEvent::HardwareSignResult {
                webview_id,
                ipc_id,
                result,
            }) => {
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
            Event::UserEvent(UserEvent::CloseWalletSelector) => {
                {
                    let mut sel = state.selector_webview_id.lock().unwrap();
                    *sel = None;
                }
                manager.close_by_label("Connect Wallet");
            }
            Event::UserEvent(UserEvent::TabAction(action)) => {
                match action {
                    TabAction::SwitchTab(i) => manager.switch_to(i),
                    TabAction::CloseTab(i) => manager.close_app(i),
                    TabAction::OpenApp { name, dist_dir } => {
                        if let Some(w) = window.as_ref() {
                            let size = w.inner_size();
                            let id = manager.next_app_id();
                            let bounds = manager.app_rect(size.width, size.height);
                            match build_app_webview(
                                w,
                                &id,
                                Some(dist_dir.clone()),
                                EmbeddedContent::Default,
                                &state,
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

            Event::NewEvents(StartCause::Init) => {
                if window.is_none() {
                    let built = WindowBuilder::new()
                        .with_title("VibeFi")
                        .with_inner_size(LogicalSize::new(1280.0, 720.0))
                        .build(event_loop_window_target)
                        .context("failed to build window");
                    let window_handle = match built {
                        Ok(window) => window,
                        Err(e) => {
                            eprintln!("window error: {e:?}");
                            *control_flow = ControlFlow::Exit;
                            return;
                        }
                    };

                    manager.set_scale_factor(window_handle.scale_factor());
                    let size = window_handle.inner_size();
                    let w = size.width;
                    let h = size.height;

                    // 1. Build tab bar
                    match build_tab_bar_webview(
                        &window_handle,
                        proxy.clone(),
                        manager.tab_bar_rect(w),
                    ) {
                        Ok(tb) => manager.tab_bar = Some(tb),
                        Err(e) => eprintln!("tab bar error: {e:?}"),
                    }

                    // 2. Build initial app webview
                    let devnet_mode = launcher.is_some();
                    let dist_dir = bundle.as_ref().map(|cfg| cfg.dist_dir.clone());
                    let embedded = if dist_dir.is_some() {
                        EmbeddedContent::Default
                    } else if devnet_mode {
                        EmbeddedContent::Launcher
                    } else {
                        EmbeddedContent::Default
                    };
                    let label = if dist_dir.is_some() {
                        "App".to_string()
                    } else if devnet_mode {
                        "Launcher".to_string()
                    } else {
                        "Home".to_string()
                    };
                    let app_id = manager.next_app_id();
                    let bounds = manager.app_rect(w, h);
                    match build_app_webview(
                        &window_handle,
                        &app_id,
                        dist_dir.clone(),
                        embedded,
                        &state,
                        proxy.clone(),
                        bounds,
                    ) {
                        Ok(wv) => {
                            manager.apps.push(AppWebViewEntry {
                                webview: wv,
                                id: app_id,
                                label,
                                dist_dir,
                            });
                            manager.active_app_index = Some(0);
                            manager.update_tab_bar();
                        }
                        Err(e) => {
                            eprintln!("webview error: {e:?}");
                            *control_flow = ControlFlow::Exit;
                            return;
                        }
                    }

                    window = Some(window_handle);
                }
            }

            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(size),
                ..
            } => {
                manager.relayout(size.width, size.height);
            }
            _ => {}
        }
    });

    Ok(())
}

struct CliArgs {
    bundle: Option<BundleConfig>,
    launcher: Option<LauncherConfig>,
    wc_project_id: Option<String>,
    wc_relay_url: Option<String>,
}

fn parse_args() -> Result<CliArgs> {
    let mut args = env::args().skip(1).peekable();
    let mut bundle_dir: Option<PathBuf> = None;
    let mut devnet_path: Option<PathBuf> = None;
    let mut devnet_mode = false;
    let mut rpc_url: Option<String> = None;
    let mut ipfs_api: Option<String> = None;
    let mut ipfs_gateway: Option<String> = None;
    let mut cache_dir: Option<PathBuf> = None;
    let mut wc_project_id: Option<String> = env::var("VIBEFI_WC_PROJECT_ID")
        .ok()
        .or_else(|| env::var("WC_PROJECT_ID").ok());
    let mut wc_relay_url: Option<String> = env::var("VIBEFI_WC_RELAY_URL")
        .ok()
        .or_else(|| env::var("WC_RELAY_URL").ok());
    let mut no_build = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bundle" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow!("--bundle requires a path"))?;
                bundle_dir = Some(PathBuf::from(value));
            }
            "--devnet" => {
                devnet_mode = true;
                if let Some(next) = args.next_if(|s| !s.starts_with("--")) {
                    devnet_path = Some(PathBuf::from(next));
                }
            }
            "--rpc" => rpc_url = args.next(),
            "--ipfs-api" => ipfs_api = args.next(),
            "--ipfs-gateway" => ipfs_gateway = args.next(),
            "--cache-dir" => cache_dir = args.next().map(PathBuf::from),
            "--wc-project-id" => wc_project_id = args.next(),
            "--wc-relay-url" => wc_relay_url = args.next(),
            "--no-build" => no_build = true,
            _ => {}
        }
    }

    let make_launcher = |devnet_path: Option<PathBuf>,
                         rpc_url: Option<String>,
                         ipfs_api: Option<String>,
                         ipfs_gateway: Option<String>,
                         cache_dir: Option<PathBuf>| {
        let default_devnet = PathBuf::from("contracts/.devnet/devnet.json");
        let resolved = devnet_path.or_else(|| {
            if default_devnet.exists() {
                Some(default_devnet)
            } else {
                None
            }
        });
        LauncherConfig {
            devnet_path: resolved,
            rpc_url: rpc_url.unwrap_or_else(|| "http://127.0.0.1:8546".to_string()),
            ipfs_api: ipfs_api.unwrap_or_else(|| "http://127.0.0.1:5001".to_string()),
            ipfs_gateway: ipfs_gateway.unwrap_or_else(|| "http://127.0.0.1:8080".to_string()),
            cache_dir: cache_dir.unwrap_or_else(|| PathBuf::from("client/.vibefi/cache")),
        }
    };

    let Some(source_dir) = bundle_dir else {
        let launcher = if devnet_mode {
            Some(make_launcher(
                devnet_path,
                rpc_url,
                ipfs_api,
                ipfs_gateway,
                cache_dir,
            ))
        } else {
            None
        };
        return Ok(CliArgs {
            bundle: None,
            launcher,
            wc_project_id,
            wc_relay_url,
        });
    };
    let source_dir = source_dir
        .canonicalize()
        .context("bundle path does not exist")?;
    let dist_dir = source_dir.join(".vibefi").join("dist");

    verify_manifest(&source_dir)?;
    if !no_build {
        build_bundle(&source_dir, &dist_dir)?;
    }

    let launcher = if devnet_mode {
        Some(make_launcher(
            devnet_path,
            rpc_url,
            ipfs_api,
            ipfs_gateway,
            cache_dir,
        ))
    } else {
        None
    };
    Ok(CliArgs {
        bundle: Some(BundleConfig {
            source_dir,
            dist_dir,
        }),
        launcher,
        wc_project_id,
        wc_relay_url,
    })
}
