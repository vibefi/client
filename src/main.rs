mod bundle;
mod config;
mod events;
mod hardware;
mod ipc;
mod ipc_contract;
mod ipfs_helper;
mod logging;
mod menu;
mod registry;
mod rpc_manager;
mod runtime_paths;
mod settings;
mod state;
mod ui_bridge;
mod walletconnect;
mod webview;
mod webview_manager;

use anyhow::{Context, Result, anyhow};
use std::{
    collections::VecDeque,
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

use bundle::{BundleConfig, build_bundle, verify_manifest};
use config::{build_network_context, load_config};
use rpc_manager::{RpcEndpoint, RpcEndpointManager};
use state::{AppState, Chain, UserEvent, WalletState};
use webview::{EmbeddedContent, WebViewHost, build_app_webview, build_tab_bar_webview};
use webview_manager::{AppWebViewEntry, AppWebViewKind, WebViewManager};

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
static SETTINGS_HTML: &str = include_str!("../internal-ui/static/settings.html");
static SETTINGS_JS: &str = include_str!("../internal-ui/dist/settings.js");
static PRELOAD_SETTINGS_JS: &str = include_str!("../internal-ui/dist/preload-settings.js");

/// Hard-coded demo private key (DO NOT USE IN PRODUCTION).
/// This matches a common dev key used across many tutorials.
pub(crate) static DEMO_PRIVKEY_HEX: &str =
    "0x59c6995e998f97a5a0044966f094538c5f0f7b4b5b5b5b5b5b5b5b5b5b5b5b5b";

fn main() -> Result<()> {
    apply_linux_env_defaults();
    logging::init_logging()?;

    let args = parse_args()?;
    let bundle = args.bundle;
    let config_path = args
        .config_path
        .or_else(|| runtime_paths::resolve_default_config());

    let network = match config_path.as_ref().map(|p| (p, load_config(p))) {
        Some((_, Ok(cfg))) => Some(build_network_context(cfg)),
        Some((path, Err(e))) => {
            tracing::warn!(path = ?path, error = %e, "failed to load config");
            None
        }
        None => None,
    };

    let initial_chain_id = network.as_ref().map(|n| n.config.chainId).unwrap_or(1);

    // --- Load user settings + build RPC manager ---
    let rpc_manager = if let Some(ref net) = network {
        let user_settings = config_path
            .as_ref()
            .map(|p| settings::load_settings(p))
            .unwrap_or_default();
        let endpoints = if user_settings.rpc_endpoints.is_empty() {
            vec![RpcEndpoint {
                url: net.rpc_url.clone(),
                label: Some("Default".to_string()),
            }]
        } else {
            user_settings.rpc_endpoints
        };
        Some(RpcEndpointManager::new(endpoints, net.http.clone()))
    } else {
        None
    };

    // --- Window + event loop ---
    let mut event_loop = tao::event_loop::EventLoopBuilder::<UserEvent>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};

        event_loop.set_activation_policy(ActivationPolicy::Regular);
        event_loop.set_dock_visibility(true);
        event_loop.set_activate_ignoring_other_apps(true);
        menu::setup_macos_app_menu("VibeFi");
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
        network,
        proxy: proxy.clone(),
        pending_connect: Arc::new(Mutex::new(VecDeque::new())),
        selector_webview_id: Arc::new(Mutex::new(None)),
        rpc_manager: Arc::new(Mutex::new(rpc_manager)),
        config_path: config_path.clone(),
        settings_webview_id: Arc::new(Mutex::new(None)),
    };
    let mut manager = WebViewManager::new(1.0);
    let mut window: Option<tao::window::Window> = None;
    #[cfg(target_os = "linux")]
    let mut gtk_tab_bar_container: Option<gtk::Box> = None;
    #[cfg(target_os = "linux")]
    let mut gtk_app_container: Option<gtk::Box> = None;

    event_loop.run(move |event, event_loop_window_target, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::Ipc { webview_id, msg }) => {
                events::user_event::handle_ipc_event(&state, &mut manager, &webview_id, msg);
            }
            Event::UserEvent(UserEvent::OpenWalletSelector) => {
                let host = window.as_ref().map(|w| WebViewHost {
                    window: w,
                    #[cfg(target_os = "linux")]
                    tab_bar_container: gtk_tab_bar_container.as_ref().unwrap(),
                    #[cfg(target_os = "linux")]
                    app_container: gtk_app_container.as_ref().unwrap(),
                });
                events::user_event::handle_open_wallet_selector(
                    host.as_ref(),
                    &state,
                    &mut manager,
                    &proxy,
                );
            }
            Event::UserEvent(UserEvent::OpenSettings) => {
                let host = window.as_ref().map(|w| WebViewHost {
                    window: w,
                    #[cfg(target_os = "linux")]
                    tab_bar_container: gtk_tab_bar_container.as_ref().unwrap(),
                    #[cfg(target_os = "linux")]
                    app_container: gtk_app_container.as_ref().unwrap(),
                });
                events::user_event::handle_open_settings(
                    host.as_ref(),
                    &state,
                    &mut manager,
                    &proxy,
                );
            }
            Event::UserEvent(UserEvent::WalletConnectPairing { uri, qr_svg }) => {
                events::user_event::handle_walletconnect_pairing(&state, &manager, uri, qr_svg);
            }
            Event::UserEvent(UserEvent::WalletConnectResult {
                webview_id,
                ipc_id,
                result,
            }) => {
                events::user_event::handle_walletconnect_result(
                    &state,
                    &mut manager,
                    webview_id,
                    ipc_id,
                    result,
                );
            }
            Event::UserEvent(UserEvent::HardwareSignResult {
                webview_id,
                ipc_id,
                result,
            }) => {
                events::user_event::handle_hardware_sign_result(
                    &manager, webview_id, ipc_id, result,
                );
            }
            Event::UserEvent(UserEvent::RpcResult {
                webview_id,
                ipc_id,
                result,
            }) => {
                events::user_event::handle_rpc_result(&manager, webview_id, ipc_id, result);
            }
            Event::UserEvent(UserEvent::ProviderEvent {
                webview_id,
                event,
                value,
            }) => {
                events::user_event::handle_provider_event(&manager, webview_id, event, value);
            }
            Event::UserEvent(UserEvent::CloseWalletSelector) => {
                events::user_event::handle_close_wallet_selector(&state, &mut manager);
            }
            Event::UserEvent(UserEvent::TabAction(action)) => {
                let host = window.as_ref().map(|w| WebViewHost {
                    window: w,
                    #[cfg(target_os = "linux")]
                    tab_bar_container: gtk_tab_bar_container.as_ref().unwrap(),
                    #[cfg(target_os = "linux")]
                    app_container: gtk_app_container.as_ref().unwrap(),
                });
                events::user_event::handle_tab_action(
                    host.as_ref(),
                    &state,
                    &mut manager,
                    &proxy,
                    action,
                );
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
                            tracing::error!(error = ?e, "window error");
                            *control_flow = ControlFlow::Exit;
                            return;
                        }
                    };

                    manager.set_scale_factor(window_handle.scale_factor());

                    #[cfg(target_os = "linux")]
                    {
                        use crate::webview_manager::TAB_BAR_HEIGHT_LOGICAL;
                        use gtk::prelude::*;
                        use tao::platform::unix::WindowExtUnix;
                        let vbox = window_handle
                            .default_vbox()
                            .expect("tao window missing default vbox on Linux");
                        let tb = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                        tb.set_size_request(-1, TAB_BAR_HEIGHT_LOGICAL as i32);
                        let app = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                        vbox.pack_start(&tb, false, true, 0);
                        vbox.pack_start(&app, true, true, 0);
                        vbox.show_all();
                        gtk_tab_bar_container = Some(tb);
                        gtk_app_container = Some(app);
                    }

                    let host = WebViewHost {
                        window: &window_handle,
                        #[cfg(target_os = "linux")]
                        tab_bar_container: gtk_tab_bar_container.as_ref().unwrap(),
                        #[cfg(target_os = "linux")]
                        app_container: gtk_app_container.as_ref().unwrap(),
                    };

                    let size = window_handle.inner_size();
                    let w = size.width;
                    let h = size.height;

                    // 1. Build tab bar
                    match build_tab_bar_webview(&host, proxy.clone(), manager.tab_bar_rect(w)) {
                        Ok(tb) => manager.tab_bar = Some(tb),
                        Err(e) => tracing::error!(error = ?e, "tab bar error"),
                    }

                    // 2. Build initial app webview
                    let has_registry = state
                        .network
                        .as_ref()
                        .map(|n| !n.config.dappRegistry.is_empty())
                        .unwrap_or(false);
                    let dist_dir = bundle.as_ref().map(|cfg| cfg.dist_dir.clone());
                    let embedded = if dist_dir.is_some() {
                        EmbeddedContent::Default
                    } else if has_registry {
                        EmbeddedContent::Launcher
                    } else {
                        EmbeddedContent::Default
                    };
                    let label = if dist_dir.is_some() {
                        "App".to_string()
                    } else if has_registry {
                        "Launcher".to_string()
                    } else {
                        "Home".to_string()
                    };
                    let kind = if has_registry && dist_dir.is_none() {
                        AppWebViewKind::Launcher
                    } else {
                        AppWebViewKind::Standard
                    };
                    let app_id = manager.next_app_id();
                    let bounds = manager.app_rect(w, h);
                    match build_app_webview(
                        &host,
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
                                kind,
                            });
                            manager.active_app_index = Some(0);
                            manager.update_tab_bar();
                        }
                        Err(e) => {
                            tracing::error!(error = ?e, "webview error");
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
    })
}

#[cfg(target_os = "linux")]
fn apply_linux_env_defaults() {
    if env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // Safety: this runs at process startup before any threads are spawned.
        unsafe {
            env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn apply_linux_env_defaults() {}

struct CliArgs {
    bundle: Option<BundleConfig>,
    config_path: Option<PathBuf>,
}

fn parse_args() -> Result<CliArgs> {
    let mut args = env::args().skip(1).peekable();
    let mut bundle_dir: Option<PathBuf> = None;
    let mut config_path: Option<PathBuf> = None;
    let mut no_build = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bundle" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow!("--bundle requires a path"))?;
                bundle_dir = Some(PathBuf::from(value));
            }
            "--config" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow!("--config requires a path"))?;
                config_path = Some(PathBuf::from(value));
            }
            "--no-build" => no_build = true,
            _ => {}
        }
    }

    let Some(source_dir) = bundle_dir else {
        return Ok(CliArgs {
            bundle: None,
            config_path,
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

    Ok(CliArgs {
        bundle: Some(BundleConfig { dist_dir }),
        config_path,
    })
}
