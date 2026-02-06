mod bundle;
mod devnet;
mod ipc;
mod menu;
mod state;
mod walletconnect;
mod webview;

use anyhow::{anyhow, bail, Context, Result};
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

use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use reqwest::blocking::Client as HttpClient;

use bundle::{build_bundle, verify_manifest, BundleConfig};
use devnet::{load_devnet, DevnetConfig, DevnetContext};
use ipc::handle_ipc;
use state::{AppState, Chain, LauncherConfig, UserEvent, WalletBackend, WalletState};
use walletconnect::{WalletConnectBridge, WalletConnectConfig};
use webview::build_webview;

static INDEX_HTML: &str = include_str!("../assets/index.html");
static LAUNCHER_HTML: &str = include_str!("../assets/launcher.html");

/// Hard-coded demo private key (DO NOT USE IN PRODUCTION).
/// This matches a common dev key used across many tutorials.
static DEMO_PRIVKEY_HEX: &str =
    "0x59c6995e998f97a5a0044966f094538c5f0f7b4b5b5b5b5b5b5b5b5b5b5b5b5b";

fn main() -> Result<()> {
    let args = parse_args()?;
    let bundle = args.bundle;
    let launcher = args.launcher;

    let devnet = launcher
        .as_ref()
        .and_then(|cfg| cfg.devnet_path.as_ref())
        .and_then(|path| load_devnet(path).ok());

    let wallet_backend = args.wallet_backend;
    let (signer, walletconnect, initial_account) = match wallet_backend {
        WalletBackend::Local => {
            eprintln!("[wallet] backend=local");
            let signer_hex = devnet
                .as_ref()
                .and_then(|cfg| cfg.developerPrivateKey.clone())
                .unwrap_or_else(|| DEMO_PRIVKEY_HEX.to_string());
            let signer: PrivateKeySigner = signer_hex
                .parse()
                .context("failed to parse signing private key")?;
            let account = format!("0x{:x}", signer.address());
            (Some(Arc::new(signer)), None, Some(account))
        }
        WalletBackend::WalletConnect => {
            eprintln!("[wallet] backend=walletconnect");
            let project_id = args
                .wc_project_id
                .or_else(|| env::var("VIBEFI_WC_PROJECT_ID").ok())
                .or_else(|| env::var("WC_PROJECT_ID").ok())
                .ok_or_else(|| {
                    anyhow!(
                        "WalletConnect backend requires --wc-project-id or VIBEFI_WC_PROJECT_ID"
                    )
                })?;
            let relay_url = args
                .wc_relay_url
                .or_else(|| env::var("VIBEFI_WC_RELAY_URL").ok())
                .or_else(|| env::var("WC_RELAY_URL").ok());
            let bridge = WalletConnectBridge::spawn(WalletConnectConfig {
                project_id,
                relay_url,
            })
            .context("failed to initialize WalletConnect bridge")?;
            (None, Some(Arc::new(Mutex::new(bridge))), None)
        }
    };

    let initial_chain_id = devnet.as_ref().map(|cfg| cfg.chainId).unwrap_or_else(|| {
        if launcher.is_some() {
            31337
        } else {
            1
        }
    });
    let state = AppState {
        wallet: Arc::new(Mutex::new(WalletState {
            authorized: false,
            chain: Chain {
                chain_id: initial_chain_id,
            },
            account: initial_account,
            walletconnect_uri: None,
        })),
        wallet_backend,
        signer,
        walletconnect,
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
        current_bundle: Arc::new(Mutex::new(bundle.as_ref().map(|cfg| cfg.dist_dir.clone()))),
    };

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
    let mut webview: Option<wry::WebView> = None;
    let mut window: Option<tao::window::Window> = None;

    event_loop.run(move |event, event_loop_window_target, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::Ipc(msg)) => {
                if let Some(webview) = webview.as_ref() {
                    if let Err(e) = handle_ipc(webview, &state, msg) {
                        eprintln!("ipc error: {e:?}");
                    }
                }
            }

            Event::NewEvents(StartCause::Init) => {
                if webview.is_none() {
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

                    let built = build_webview(
                        &window_handle,
                        state.clone(),
                        proxy.clone(),
                        bundle.clone(),
                        launcher.is_some(),
                    );
                    let webview_handle = match built {
                        Ok(webview) => webview,
                        Err(e) => {
                            eprintln!("webview error: {e:?}");
                            *control_flow = ControlFlow::Exit;
                            return;
                        }
                    };

                    window = Some(window_handle);
                    webview = Some(webview_handle);
                }
            }

            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });

    Ok(())
}

struct CliArgs {
    bundle: Option<BundleConfig>,
    launcher: Option<LauncherConfig>,
    wallet_backend: WalletBackend,
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
    let mut wallet_backend = env::var("VIBEFI_WALLET_BACKEND")
        .ok()
        .as_deref()
        .map(parse_wallet_backend)
        .transpose()?
        .unwrap_or(WalletBackend::Local);
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
            "--wallet" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow!("--wallet requires one of: local, walletconnect"))?;
                wallet_backend = parse_wallet_backend(&value)?;
            }
            "--walletconnect" => wallet_backend = WalletBackend::WalletConnect,
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
            wallet_backend,
            wc_project_id: wc_project_id.clone(),
            wc_relay_url: wc_relay_url.clone(),
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
            wallet_backend,
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
        wallet_backend,
        wc_project_id,
        wc_relay_url,
    })
}

fn parse_wallet_backend(value: &str) -> Result<WalletBackend> {
    match value {
        "local" => Ok(WalletBackend::Local),
        "walletconnect" | "wc" => Ok(WalletBackend::WalletConnect),
        _ => bail!(
            "unsupported wallet backend '{}'; expected local|walletconnect",
            value
        ),
    }
}
