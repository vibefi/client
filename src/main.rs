mod bundle;
mod code;
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

use anyhow::{Context, Result};
use clap::Parser;
use std::{
    collections::HashMap,
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tao::{
    dpi::LogicalSize,
    event::{Event, StartCause, WindowEvent},
    event_loop::ControlFlow,
    window::WindowBuilder,
};

use bundle::{BundleConfig, build_bundle, verify_manifest};
use config::{CliArgs, ConfigBuilder, load_config};
use rpc_manager::{DEFAULT_MAX_CONCURRENT_RPC, RpcEndpoint, RpcEndpointManager};
use state::{AppState, Chain, UserEvent, WalletState};
use webview::{EmbeddedContent, WebViewHost, build_app_webview, build_tab_bar_webview};
use webview_manager::{AppWebViewEntry, AppWebViewKind, WebViewManager};

static CODE_HTML: &str = include_str!("../internal-ui/static/code.html");
static INDEX_HTML: &str = include_str!("../internal-ui/static/home.html");
static LAUNCHER_HTML: &str = include_str!("../internal-ui/static/launcher.html");
static TAB_BAR_HTML: &str = include_str!("../internal-ui/static/tabbar.html");
static WALLET_SELECTOR_HTML: &str = include_str!("../internal-ui/static/wallet-selector.html");
static CODE_JS: &str = include_str!("../internal-ui/dist/code.js");
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

fn main() -> Result<()> {
    apply_linux_env_defaults();
    logging::init_logging()?;

    let cli = CliArgs::parse();
    let bundle = resolve_bundle(&cli)?;
    let studio_bundle = if bundle.is_some() {
        if cli.studio_bundle.is_some() {
            tracing::warn!("--studio-bundle is ignored when --bundle is provided");
        }
        None
    } else {
        resolve_studio_bundle(&cli)?
    };
    let config_path = cli
        .config
        .or_else(|| runtime_paths::resolve_default_config());

    let resolved = match config_path.as_ref().map(|p| (p, load_config(p))) {
        Some((_, Ok(cfg))) => {
            let resolved = ConfigBuilder::new(cfg, config_path.clone()).build();
            resolved.log_startup_summary();
            Some(Arc::new(resolved))
        }
        Some((path, Err(e))) => {
            tracing::warn!(path = ?path, error = %e, "failed to load config");
            None
        }
        None => None,
    };

    let initial_chain_id = resolved.as_ref().map(|r| r.chain_id).unwrap_or(1);

    // --- Load user settings + build RPC manager ---
    let rpc_manager = if let Some(ref res) = resolved {
        let user_settings = res
            .config_path
            .as_ref()
            .map(|p| settings::load_settings(p))
            .unwrap_or_default();
        let endpoints = if user_settings.rpc_endpoints.is_empty() {
            vec![RpcEndpoint {
                url: res.rpc_url.clone(),
                label: Some("Default".to_string()),
            }]
        } else {
            user_settings.rpc_endpoints
        };
        let max_concurrent = user_settings
            .max_concurrent_rpc
            .unwrap_or(DEFAULT_MAX_CONCURRENT_RPC);
        Some(RpcEndpointManager::new(
            endpoints,
            res.http_client.clone(),
            max_concurrent,
        ))
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
    let code_workspace_root = code::project::resolve_workspace_root();
    if let Err(err) = std::fs::create_dir_all(&code_workspace_root) {
        tracing::warn!(
            path = %code_workspace_root.display(),
            error = %err,
            "failed to create code workspace root"
        );
    }

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
        resolved,
        proxy: proxy.clone(),
        pending_connect: Arc::new(Mutex::new(VecDeque::new())),
        app_capabilities: Arc::new(Mutex::new(HashMap::new())),
        code: Arc::new(Mutex::new(state::CodeState {
            active_project: None,
            workspace_root: code_workspace_root,
            dev_server: None,
            next_dev_server_id: 1,
        })),
        selector_webview_id: Arc::new(Mutex::new(None)),
        rpc_manager: Arc::new(Mutex::new(rpc_manager)),
        settings_webview_id: Arc::new(Mutex::new(None)),
        pending_rpc_counts: Arc::new(Mutex::new(HashMap::new())),
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
                    tab_bar_container: gtk_tab_bar_container
                        .as_ref()
                        .expect("linux tab bar container not initialized"),
                    #[cfg(target_os = "linux")]
                    app_container: gtk_app_container
                        .as_ref()
                        .expect("linux app container not initialized"),
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
                    tab_bar_container: gtk_tab_bar_container
                        .as_ref()
                        .expect("linux tab bar container not initialized"),
                    #[cfg(target_os = "linux")]
                    app_container: gtk_app_container
                        .as_ref()
                        .expect("linux app container not initialized"),
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
            Event::UserEvent(UserEvent::RpcPendingChanged { webview_id, count }) => {
                events::user_event::handle_rpc_pending_changed(&manager, &webview_id, count);
            }
            Event::UserEvent(UserEvent::RpcResult {
                webview_id,
                ipc_id,
                result,
            }) => {
                events::user_event::handle_rpc_result(&manager, webview_id.clone(), ipc_id, result);
                let count = state.decrement_rpc_pending(&webview_id);
                events::user_event::handle_rpc_pending_changed(&manager, &webview_id, count);
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
                    tab_bar_container: gtk_tab_bar_container
                        .as_ref()
                        .expect("linux tab bar container not initialized"),
                    #[cfg(target_os = "linux")]
                    app_container: gtk_app_container
                        .as_ref()
                        .expect("linux app container not initialized"),
                });
                events::user_event::handle_tab_action(
                    host.as_ref(),
                    &state,
                    &mut manager,
                    &proxy,
                    action,
                );
            }
            Event::UserEvent(UserEvent::StudioBundleResolved {
                placeholder_id,
                result,
            }) => {
                let Some(index) = manager.apps.iter().position(|entry| entry.id == placeholder_id)
                else {
                    tracing::warn!(
                        placeholder_id = %placeholder_id,
                        "studio placeholder tab not found"
                    );
                    return;
                };

                match result {
                    Ok(dist_dir) => {
                        let Some(window_ref) = window.as_ref() else {
                            tracing::warn!("window missing while resolving studio bundle");
                            return;
                        };
                        let host = WebViewHost {
                            window: window_ref,
                            #[cfg(target_os = "linux")]
                            tab_bar_container: gtk_tab_bar_container
                                .as_ref()
                                .expect("linux tab bar container not initialized"),
                            #[cfg(target_os = "linux")]
                            app_container: gtk_app_container
                                .as_ref()
                                .expect("linux app container not initialized"),
                        };
                        let size = window_ref.inner_size();
                        let bounds = manager.app_rect(size.width, size.height);
                        match build_app_webview(
                            &host,
                            &placeholder_id,
                            Some(dist_dir.clone()),
                            EmbeddedContent::Default,
                            &state,
                            proxy.clone(),
                            bounds,
                            AppWebViewKind::Studio,
                        ) {
                            Ok(studio_webview) => {
                                let was_active = manager.active_app_index == Some(index);
                                if !was_active {
                                    if let Err(err) = studio_webview.set_visible(false) {
                                        tracing::warn!(
                                            error = %err,
                                            "failed to hide inactive studio webview"
                                        );
                                    }
                                }

                                if let Ok(mut caps) = state.app_capabilities.lock() {
                                    let studio_caps =
                                        events::user_event::load_app_capabilities_from_dist(
                                            &dist_dir,
                                        );
                                    caps.insert(placeholder_id.clone(), studio_caps);
                                } else {
                                    tracing::warn!(
                                        "failed to acquire app_capabilities lock for studio tab"
                                    );
                                }

                                manager.apps[index] = AppWebViewEntry {
                                    webview: studio_webview,
                                    id: placeholder_id,
                                    label: "Studio".to_string(),
                                    kind: AppWebViewKind::Studio,
                                    source_dir: None,
                                    selectable: true,
                                    loading: false,
                                };
                                manager.update_tab_bar();
                            }
                            Err(err) => {
                                tracing::warn!(error = %err, "failed to build studio webview");
                                if let Some(entry) = manager.apps.get_mut(index) {
                                    entry.label = "Studio (unavailable)".to_string();
                                    entry.selectable = false;
                                    entry.loading = false;
                                }
                                manager.update_tab_bar();
                            }
                        }
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "failed to resolve studio dapp bundle");
                        if let Some(entry) = manager.apps.get_mut(index) {
                            entry.label = "Studio (unavailable)".to_string();
                            entry.selectable = false;
                            entry.loading = false;
                        }
                        manager.update_tab_bar();
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
                            tracing::error!(error = ?e, "window error");
                            *control_flow = ControlFlow::Exit;
                            return;
                        }
                    };
                    #[cfg(target_os = "macos")]
                    menu::setup_macos_dock_icon();

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
                        tab_bar_container: gtk_tab_bar_container
                            .as_ref()
                            .expect("linux tab bar container not initialized"),
                        #[cfg(target_os = "linux")]
                        app_container: gtk_app_container
                            .as_ref()
                            .expect("linux app container not initialized"),
                    };

                    let size = window_handle.inner_size();
                    let w = size.width;
                    let h = size.height;

                    // 1. Build tab bar
                    let enable_devtools = state
                        .resolved
                        .as_ref()
                        .map(|r| r.enable_devtools)
                        .unwrap_or(cfg!(debug_assertions));
                    match build_tab_bar_webview(
                        &host,
                        proxy.clone(),
                        manager.tab_bar_rect(w),
                        enable_devtools,
                    ) {
                        Ok(tb) => manager.tab_bar = Some(tb),
                        Err(e) => tracing::error!(error = ?e, "tab bar error"),
                    }

                    // 2. Build initial app webview(s)
                    let has_registry = state
                        .resolved
                        .as_ref()
                        .map(|r| !r.dapp_registry.is_empty())
                        .unwrap_or(false);
                    let dist_dir = bundle.as_ref().map(|cfg| cfg.dist_dir.clone());
                    let bundle_source_dir = bundle.as_ref().map(|cfg| cfg.source_dir.clone());
                    let studio_dist_dir = studio_bundle.as_ref().map(|cfg| cfg.dist_dir.clone());
                    let bounds = manager.app_rect(w, h);
                    if let Some(dist_dir) = dist_dir.clone() {
                        let source_dir = bundle_source_dir.clone();
                        let app_id = manager.next_app_id();
                        match build_app_webview(
                            &host,
                            &app_id,
                            Some(dist_dir),
                            EmbeddedContent::Default,
                            &state,
                            proxy.clone(),
                            bounds,
                            AppWebViewKind::Standard,
                        ) {
                            Ok(wv) => {
                                manager.apps.push(AppWebViewEntry {
                                    webview: wv,
                                    id: app_id,
                                    label: "App".to_string(),
                                    kind: AppWebViewKind::Standard,
                                    source_dir,
                                    selectable: true,
                                    loading: false,
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
                    } else if has_registry {
                        let launcher_id = manager.next_app_id();
                        let launcher_webview = match build_app_webview(
                            &host,
                            &launcher_id,
                            None,
                            EmbeddedContent::Launcher,
                            &state,
                            proxy.clone(),
                            bounds,
                            AppWebViewKind::Launcher,
                        ) {
                            Ok(wv) => wv,
                            Err(e) => {
                                tracing::error!(error = ?e, "launcher webview error");
                                *control_flow = ControlFlow::Exit;
                                return;
                            }
                        };

                        manager.apps.push(AppWebViewEntry {
                            webview: launcher_webview,
                            id: launcher_id,
                            label: "Launcher".to_string(),
                            kind: AppWebViewKind::Launcher,
                            source_dir: None,
                            selectable: true,
                            loading: false,
                        });
                        manager.active_app_index = Some(0);

                        let studio_placeholder_id = manager.next_app_id();
                        let studio_placeholder = match build_app_webview(
                            &host,
                            &studio_placeholder_id,
                            None,
                            EmbeddedContent::Default,
                            &state,
                            proxy.clone(),
                            bounds,
                            AppWebViewKind::Studio,
                        ) {
                            Ok(wv) => wv,
                            Err(e) => {
                                tracing::error!(error = ?e, "studio placeholder webview error");
                                *control_flow = ControlFlow::Exit;
                                return;
                            }
                        };
                        if let Err(err) = studio_placeholder.set_visible(false) {
                            tracing::warn!(
                                error = %err,
                                "failed to hide inactive studio placeholder tab"
                            );
                        }
                        manager.apps.push(AppWebViewEntry {
                            webview: studio_placeholder,
                            id: studio_placeholder_id.clone(),
                            label: "Studio".to_string(),
                            kind: AppWebViewKind::Studio,
                            source_dir: None,
                            selectable: false,
                            loading: true,
                        });

                        let code_id = manager.next_app_id();
                        let code_webview = match build_app_webview(
                            &host,
                            &code_id,
                            None,
                            EmbeddedContent::Code,
                            &state,
                            proxy.clone(),
                            bounds,
                            AppWebViewKind::Code,
                        ) {
                            Ok(wv) => wv,
                            Err(e) => {
                                tracing::error!(error = ?e, "code webview error");
                                *control_flow = ControlFlow::Exit;
                                return;
                            }
                        };
                        if let Err(err) = code_webview.set_visible(false) {
                            tracing::warn!(error = %err, "failed to hide inactive code webview");
                        }
                        manager.apps.push(AppWebViewEntry {
                            webview: code_webview,
                            id: code_id,
                            label: "Code".to_string(),
                            kind: AppWebViewKind::Code,
                            source_dir: None,
                            selectable: true,
                            loading: false,
                        });

                        manager.update_tab_bar();

                        let state_clone = state.clone();
                        let proxy_clone = proxy.clone();
                        let studio_placeholder_id_clone = studio_placeholder_id.clone();
                        std::thread::spawn(move || {
                            let result = (|| -> Result<std::path::PathBuf> {
                                if let Some(studio_dist_dir) = studio_dist_dir {
                                    tracing::info!(
                                        studio_dist_dir = %studio_dist_dir.display(),
                                        "loading Studio from local --studio-bundle"
                                    );
                                    return Ok(studio_dist_dir);
                                }
                                let studio_dapp_id = state_clone
                                    .resolved
                                    .as_ref()
                                    .and_then(|resolved| resolved.studio_dapp_id)
                                    .ok_or_else(|| {
                                        anyhow::anyhow!("config missing studioDappId")
                                    })?;
                                let studio_cid = registry::resolve_published_root_cid_by_dapp_id(
                                    &state_clone,
                                    studio_dapp_id,
                                )?;
                                tracing::info!(
                                    dapp_id = studio_dapp_id,
                                    cid = %studio_cid,
                                    "loading Studio from DappRegistry"
                                );
                                registry::prepare_dapp_dist(&state_clone, &studio_cid, None)
                            })()
                            .map_err(|err| err.to_string());
                            let _ = proxy_clone.send_event(UserEvent::StudioBundleResolved {
                                placeholder_id: studio_placeholder_id_clone,
                                result,
                            });
                        });
                    } else {
                        let app_id = manager.next_app_id();
                        match build_app_webview(
                            &host,
                            &app_id,
                            None,
                            EmbeddedContent::Code,
                            &state,
                            proxy.clone(),
                            bounds,
                            AppWebViewKind::Code,
                        ) {
                            Ok(wv) => {
                                manager.apps.push(AppWebViewEntry {
                                    webview: wv,
                                    id: app_id,
                                    label: "Code".to_string(),
                                    kind: AppWebViewKind::Code,
                                    source_dir: None,
                                    selectable: true,
                                    loading: false,
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
            Event::LoopDestroyed => {
                if let Err(err) = code::dev_server::stop_dev_server_for_shutdown(&state) {
                    tracing::warn!(error = %err, "failed to stop Code dev server during shutdown");
                }
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
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // Safety: this runs at process startup before any threads are spawned.
        unsafe {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn apply_linux_env_defaults() {}

fn resolve_bundle(cli: &CliArgs) -> Result<Option<BundleConfig>> {
    let Some(ref source) = cli.bundle else {
        return Ok(None);
    };
    let source_dir = source
        .canonicalize()
        .context("bundle path does not exist")?;
    let dist_dir = source_dir.join(".vibefi").join("dist");
    verify_manifest(&source_dir)?;
    if !cli.no_build {
        build_bundle(&source_dir, &dist_dir)?;
    }
    Ok(Some(BundleConfig {
        source_dir,
        dist_dir,
    }))
}

fn resolve_studio_bundle(cli: &CliArgs) -> Result<Option<BundleConfig>> {
    let Some(ref source) = cli.studio_bundle else {
        return Ok(None);
    };
    let source_dir = source
        .canonicalize()
        .context("studio bundle path does not exist")?;
    let dist_dir = source_dir.join(".vibefi").join("dist");
    verify_manifest(&source_dir)?;
    if !cli.no_build {
        build_bundle(&source_dir, &dist_dir)?;
    }
    Ok(Some(BundleConfig {
        source_dir,
        dist_dir,
    }))
}
