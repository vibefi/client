use anyhow::{anyhow, Context, Result};
use alloy_primitives::{Address, B256, Bytes, Log, U256};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    env,
    fs,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
    sync::{Arc, Mutex},
};
use tao::{
    dpi::LogicalSize,
    event::{Event, StartCause, WindowEvent},
    event_loop::ControlFlow,
    window::WindowBuilder,
};
use wry::{
    http::{header::CONTENT_TYPE, Response},
    WebView, WebViewBuilder,
};

use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use alloy_sol_types::{sol, SolEvent};
use mime_guess::MimeGuess;
use reqwest::blocking::{Client as HttpClient, Response as HttpResponse};
use reqwest::blocking::multipart::{Form, Part};

static INDEX_HTML: &str = include_str!("../assets/index.html");
static LAUNCHER_HTML: &str = include_str!("../assets/launcher.html");

/// Hard-coded demo private key (DO NOT USE IN PRODUCTION).
/// This matches a common dev key used across many tutorials.
static DEMO_PRIVKEY_HEX: &str = "0x59c6995e998f97a5a0044966f094538c5f0f7b4b5b5b5b5b5b5b5b5b5b5b5b5b";

#[derive(Debug, Clone, Copy)]
struct Chain {
    chain_id: u64,
}

impl Default for Chain {
    fn default() -> Self {
        // Ethereum mainnet
        Self { chain_id: 1 }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IpcRequest {
    id: u64,
    #[serde(default)]
    provider_id: Option<String>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Clone)]
enum UserEvent {
    Ipc(String),
}

#[derive(Debug, Serialize)]
struct ProviderInfo {
    name: &'static str,
    // EIP-1193: the chainId should be hex string.
    chain_id: String,
}

#[derive(Debug, Default)]
struct WalletState {
    // Whether the dapp has been granted access to accounts.
    authorized: bool,
    chain: Chain,
}

#[derive(Clone)]
struct AppState {
    wallet: Arc<Mutex<WalletState>>,
    signer: Arc<PrivateKeySigner>,
    devnet: Option<DevnetContext>,
    current_bundle: Arc<Mutex<Option<PathBuf>>>,
}

impl AppState {
    fn address(&self) -> Address {
        self.signer.address()
    }

    fn get_address(&self) -> String {
        format!("{:?}", self.signer.address())
    }

    fn chain_id_hex(&self) -> String {
        let chain_id = self.wallet.lock().unwrap().chain.chain_id;
        format!("0x{:x}", chain_id)
    }
}

static INIT_SCRIPT: Lazy<String> = Lazy::new(|| {
    // A minimal EIP-1193 provider shim.
    // - ethereum.request({method, params}) -> Promise
    // - events: on/off/removeListener
    // - emits connect, chainChanged, accountsChanged
    // - no outbound network; requests go to Rust via IPC
    r#"
(() => {
  const PROVIDER_ID = 'wry-demo-wallet';
  const callbacks = new Map();
  let nextId = 1;

  // Lightweight event emitter
  const listeners = new Map();
  function on(event, handler) {
    if (typeof handler !== 'function') return;
    const set = listeners.get(event) ?? new Set();
    set.add(handler);
    listeners.set(event, set);
  }
  function off(event, handler) {
    const set = listeners.get(event);
    if (!set) return;
    set.delete(handler);
  }
  function emit(event, ...args) {
    const set = listeners.get(event);
    if (!set) return;
    for (const h of Array.from(set)) {
      try { h(...args); } catch (_) {}
    }
  }

  // Expose a controlled hook for Rust -> JS event emission.
  // NOTE: Do not expose this on window directly in production.
  window.__WryEthereumEmit = (event, payload) => {
    emit(event, payload);
  };

  async function request({ method, params }) {
    return new Promise((resolve, reject) => {
      const id = nextId++;
      callbacks.set(id, { resolve, reject });
      window.ipc.postMessage(JSON.stringify({
        id,
        providerId: PROVIDER_ID,
        method,
        params: params ?? []
      }));
    });
  }

  function handleResponse(id, result, error) {
    const cb = callbacks.get(id);
    if (!cb) return;
    callbacks.delete(id);
    if (error) cb.reject(error);
    else cb.resolve(result);
  }

  // Rust calls this to resolve/reject pending requests.
  window.__WryEthereumResolve = (id, result, error) => {
    handleResponse(id, result ?? null, error ?? null);
  };

  // EIP-1193-ish provider object
  const ethereum = {
    isWry: true,
    isMetaMask: false,
    // EIP-1193
    request,
    // event api (common wallet compat)
    on,
    removeListener: off,
    off,
    // legacy-ish compatibility
    enable: () => request({ method: 'eth_requestAccounts', params: [] }),
  };

  // define it early
  if (!window.ethereum) {
    Object.defineProperty(window, 'ethereum', {
      value: ethereum,
      configurable: false,
      enumerable: true,
      writable: false
    });
  }

  // Minimal vibefi launcher API for non-provider UI actions.
  window.vibefi = {
    request: ({ method, params }) => new Promise((resolve, reject) => {
      const id = nextId++;
      callbacks.set(id, { resolve, reject });
      window.ipc.postMessage(JSON.stringify({
        id,
        providerId: 'vibefi-launcher',
        method,
        params: params ?? []
      }));
    })
  };

  // Signal a connect event once the page is ready.
  // Wallets often emit connect as soon as injected.
  Promise.resolve().then(async () => {
    try {
      const chainId = await request({ method: 'eth_chainId', params: [] });
      emit('connect', { chainId });
    } catch (_) {}
  });
})();
"#
    .to_string()
});

#[derive(Debug, Clone)]
struct BundleConfig {
    source_dir: PathBuf,
    dist_dir: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
struct BundleManifest {
    files: Vec<BundleManifestFile>,
}

#[derive(Debug, Deserialize, Serialize)]
struct BundleManifestFile {
    path: String,
    bytes: u64,
}

#[derive(Debug, Deserialize, Clone)]
struct DevnetConfig {
    chainId: u64,
    deployBlock: Option<u64>,
    dappRegistry: String,
    developerPrivateKey: Option<String>,
}

#[derive(Debug, Clone)]
struct DevnetContext {
    config: DevnetConfig,
    rpc_url: String,
    ipfs_api: String,
    ipfs_gateway: String,
    cache_dir: PathBuf,
    http: HttpClient,
}

#[derive(Debug, Clone, Serialize)]
struct DappInfo {
    dappId: String,
    versionId: String,
    name: String,
    version: String,
    description: String,
    status: String,
    rootCid: String,
}

#[derive(Debug)]
struct LauncherConfig {
    devnet_path: Option<PathBuf>,
    rpc_url: String,
    ipfs_api: String,
    ipfs_gateway: String,
    cache_dir: PathBuf,
}

fn main() -> Result<()> {
    let (bundle, launcher) = parse_args()?;

    // --- Build signing wallet (Alloy) ---
    let devnet = launcher
        .as_ref()
        .and_then(|cfg| cfg.devnet_path.as_ref())
        .and_then(|path| load_devnet(path).ok());
    let signer_hex = devnet
        .as_ref()
        .and_then(|cfg| cfg.developerPrivateKey.clone())
        .unwrap_or_else(|| DEMO_PRIVKEY_HEX.to_string());
    let signer: PrivateKeySigner = signer_hex
        .parse()
        .context("failed to parse signing private key")?;

    let initial_chain_id = devnet
        .as_ref()
        .map(|cfg| cfg.chainId)
        .unwrap_or_else(|| if launcher.is_some() { 31337 } else { 1 });
    let state = AppState {
        wallet: Arc::new(Mutex::new(WalletState {
            authorized: false,
            chain: Chain {
                chain_id: initial_chain_id,
            },
        })),
        signer: Arc::new(signer),
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
    let mut event_loop =
        tao::event_loop::EventLoopBuilder::<UserEvent>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};

        event_loop.set_activation_policy(ActivationPolicy::Regular);
        event_loop.set_dock_visibility(true);
        event_loop.set_activate_ignoring_other_apps(true);
        setup_macos_app_menu("Wry EIP-1193 demo");
    }
    let proxy = event_loop.create_proxy();
    let mut webview: Option<WebView> = None;
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

fn build_webview(
    window: &tao::window::Window,
    state: AppState,
    proxy: tao::event_loop::EventLoopProxy<UserEvent>,
    bundle: Option<BundleConfig>,
    devnet_mode: bool,
) -> Result<WebView> {
    let wallet_state = state.wallet.clone();
    let current_bundle = state.current_bundle.clone();

    // Serve only our embedded assets.
    let protocol_bundle = bundle.clone();
    let protocol = move |_webview_id: wry::WebViewId, request: wry::http::Request<Vec<u8>>| {
        let path = request.uri().path();
        let active_bundle = current_bundle.lock().unwrap().clone().or_else(|| {
            protocol_bundle
                .as_ref()
                .map(|cfg| cfg.dist_dir.clone())
        });
        let (body, mime) = if let Some(dist_dir) = active_bundle {
            let rel = path.trim_start_matches('/');
            let mut file_path = if rel.is_empty() {
                dist_dir.join("index.html")
            } else {
                dist_dir.join(rel)
            };
            if file_path.is_dir() {
                file_path = file_path.join("index.html");
            }
            if !file_path.exists() {
                (
                    format!("Not found: {path}").into_bytes(),
                    "text/plain; charset=utf-8".to_string(),
                )
            } else {
                let data = fs::read(&file_path).unwrap_or_else(|_| Vec::new());
                let guess = MimeGuess::from_path(&file_path)
                    .first_or_octet_stream()
                    .essence_str()
                    .to_string();
                (data, guess)
            }
        } else {
            match path {
                "/" | "/index.html" => {
                    if devnet_mode {
                        (LAUNCHER_HTML.as_bytes().to_vec(), "text/html; charset=utf-8".to_string())
                    } else {
                        (INDEX_HTML.as_bytes().to_vec(), "text/html; charset=utf-8".to_string())
                    }
                }
                _ => (format!("Not found: {path}").into_bytes(), "text/plain; charset=utf-8".to_string()),
            }
        };

        Response::builder()
            .status(200)
            .header(CONTENT_TYPE, mime.as_str())
            // harden: disallow loading remote resources via CSP.
            .header(
                "Content-Security-Policy",
                "default-src 'self' app:; img-src 'self' data: app:; style-src 'self' 'unsafe-inline' app:; script-src 'self' 'unsafe-inline' app:; connect-src 'none'; frame-src 'none'",
            )
            .body(std::borrow::Cow::Owned(body)).unwrap()
    };

    let navigation_handler = |url: String| {
        // HARD BLOCK: no http/https/file navigation.
        // Allow only our custom scheme + a couple of benign internal URLs.
        url.starts_with("app://") || url == "about:blank"
    };

    let proxy = proxy;

    let webview = WebViewBuilder::new()
        .with_initialization_script((*INIT_SCRIPT).clone())
        .with_custom_protocol("app".into(), protocol)
        .with_url("app://index.html")
        .with_navigation_handler(navigation_handler)
        .with_ipc_handler(move |req: wry::http::Request<String>| {
            // Forward to the main event loop so we can respond using the WebView handle.
            let _ = proxy.send_event(UserEvent::Ipc(req.body().clone()));
        })
        .build(window)
        .context("failed to build webview")?;

    // Emit initial chain/accounts state after load.
    // Some dapps rely on accountsChanged/chainChanged events.
    let addr = state.address();
    let chain_hex = state.chain_id_hex();
    {
        let ws = wallet_state.lock().unwrap();
        if ws.authorized {
            emit_accounts_changed(&webview, vec![addr]);
        }
    }
    emit_chain_changed(&webview, chain_hex);

    Ok(webview)
}

fn parse_args() -> Result<(Option<BundleConfig>, Option<LauncherConfig>)> {
    let mut args = env::args().skip(1).peekable();
    let mut bundle_dir: Option<PathBuf> = None;
    let mut devnet_path: Option<PathBuf> = None;
    let mut devnet_mode = false;
    let mut rpc_url: Option<String> = None;
    let mut ipfs_api: Option<String> = None;
    let mut ipfs_gateway: Option<String> = None;
    let mut cache_dir: Option<PathBuf> = None;
    let mut no_build = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bundle" => {
                let value = args.next().ok_or_else(|| anyhow!("--bundle requires a path"))?;
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
            "--no-build" => no_build = true,
            _ => {}
        }
    }

    let Some(source_dir) = bundle_dir else {
        let launcher = if devnet_mode {
            let default_devnet = PathBuf::from("contracts/.devnet/devnet.json");
            let resolved = devnet_path.or_else(|| {
                if default_devnet.exists() {
                    Some(default_devnet)
                } else {
                    None
                }
            });
            Some(LauncherConfig {
                devnet_path: resolved,
                rpc_url: rpc_url.unwrap_or_else(|| "http://127.0.0.1:8546".to_string()),
                ipfs_api: ipfs_api.unwrap_or_else(|| "http://127.0.0.1:5001".to_string()),
                ipfs_gateway: ipfs_gateway.unwrap_or_else(|| "http://127.0.0.1:8080".to_string()),
                cache_dir: cache_dir.unwrap_or_else(|| PathBuf::from("client/.vibefi/cache")),
            })
        } else {
            None
        };
        return Ok((None, launcher));
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
        let default_devnet = PathBuf::from("contracts/.devnet/devnet.json");
        let resolved = devnet_path.or_else(|| {
            if default_devnet.exists() {
                Some(default_devnet)
            } else {
                None
            }
        });
        Some(LauncherConfig {
            devnet_path: resolved,
            rpc_url: rpc_url.unwrap_or_else(|| "http://127.0.0.1:8546".to_string()),
            ipfs_api: ipfs_api.unwrap_or_else(|| "http://127.0.0.1:5001".to_string()),
            ipfs_gateway: ipfs_gateway.unwrap_or_else(|| "http://127.0.0.1:8080".to_string()),
            cache_dir: cache_dir.unwrap_or_else(|| PathBuf::from("client/.vibefi/cache")),
        })
    } else {
        None
    };
    Ok((Some(BundleConfig { source_dir, dist_dir }), launcher))
}

fn verify_manifest(bundle_dir: &Path) -> Result<()> {
    let manifest_path = bundle_dir.join("manifest.json");
    if !manifest_path.exists() {
        return Err(anyhow!("manifest.json missing in bundle"));
    }
    let content = fs::read_to_string(&manifest_path).context("read manifest.json")?;
    let manifest: BundleManifest = serde_json::from_str(&content).context("parse manifest.json")?;
    for entry in manifest.files {
        let file_path = bundle_dir.join(&entry.path);
        if !file_path.exists() {
            return Err(anyhow!("bundle missing file {}", entry.path));
        }
        let meta = fs::metadata(&file_path).context("stat bundle file")?;
        if meta.len() != entry.bytes {
            return Err(anyhow!(
                "bundle file size mismatch {} expected {} got {}",
                entry.path,
                entry.bytes,
                meta.len()
            ));
        }
    }
    Ok(())
}

const STANDARD_PACKAGE_JSON: &str = r#"{
  "name": "vibefi-dapp",
  "private": true,
  "version": "0.0.1",
  "type": "module",
  "dependencies": {
    "react": "19.2.4",
    "react-dom": "19.2.4",
    "wagmi": "3.4.1",
    "viem": "2.45.0",
    "shadcn": "3.7.0"
  },
  "devDependencies": {
    "@vitejs/plugin-react": "5.1.2",
    "typescript": "5.9.3",
    "vite": "7.2.4"
  }
}
"#;

const STANDARD_VITE_CONFIG: &str = r#"import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
});
"#;

const STANDARD_TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ES2022",
    "useDefineForClassFields": true,
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "Bundler",
    "allowImportingTsExtensions": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx",
    "strict": true
  },
  "include": ["src"]
}
"#;

fn write_standard_build_files(bundle_dir: &Path) -> Result<()> {
    fs::write(bundle_dir.join("package.json"), STANDARD_PACKAGE_JSON)?;
    fs::write(bundle_dir.join("vite.config.ts"), STANDARD_VITE_CONFIG)?;
    fs::write(bundle_dir.join("tsconfig.json"), STANDARD_TSCONFIG)?;
    Ok(())
}

fn build_bundle(bundle_dir: &Path, dist_dir: &Path) -> Result<()> {
    write_standard_build_files(bundle_dir)?;

    let node_modules = bundle_dir.join("node_modules");
    if !node_modules.exists() {
        let status = Command::new("bun")
            .arg("install")
            .arg("--no-save")
            .current_dir(bundle_dir)
            .status()
            .context("bun install failed")?;
        if !status.success() {
            return Err(anyhow!("bun install failed"));
        }
    }

    fs::create_dir_all(dist_dir).context("create dist dir")?;
    // Use relative path from bundle_dir for vite's outDir since vite runs in bundle_dir
    let relative_dist = PathBuf::from(".vibefi").join("dist");
    let status = Command::new("bun")
        .arg("x")
        .arg("vite")
        .arg("build")
        .arg("--emptyOutDir")
        .arg("--outDir")
        .arg(&relative_dist)
        .current_dir(bundle_dir)
        .status()
        .context("bun vite build failed")?;
    if !status.success() {
        return Err(anyhow!("bun vite build failed"));
    }
    Ok(())
}

fn load_devnet(path: &Path) -> Result<DevnetConfig> {
    let raw = fs::read_to_string(path).context("read devnet.json")?;
    let cfg: DevnetConfig = serde_json::from_str(&raw).context("parse devnet.json")?;
    Ok(cfg)
}

fn is_rpc_passthrough(method: &str) -> bool {
    matches!(
        method,
        "eth_blockNumber"
            | "eth_getBlockByNumber"
            | "eth_getBlockByHash"
            | "eth_getBalance"
            | "eth_getCode"
            | "eth_getLogs"
            | "eth_call"
            | "eth_estimateGas"
            | "eth_gasPrice"
            | "eth_feeHistory"
            | "eth_maxPriorityFeePerGas"
            | "eth_getTransactionReceipt"
            | "eth_getTransactionByHash"
            | "eth_getStorageAt"
            | "eth_getTransactionCount"
            | "eth_sendRawTransaction"
    )
}

fn proxy_rpc(state: &AppState, req: &IpcRequest) -> Result<Value> {
    let devnet = state.devnet.as_ref().ok_or_else(|| anyhow!("Devnet not configured"))?;
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": req.method,
        "params": req.params,
    });

    // Log RPC request
    println!("[RPC] -> {} params={}", req.method, serde_json::to_string(&req.params).unwrap_or_default());

    let res = devnet
        .http
        .post(&devnet.rpc_url)
        .json(&payload)
        .send()
        .context("rpc request failed")?;
    let v: Value = res.json().context("rpc decode failed")?;

    // Log RPC response (truncate if too long)
    let result_str = v.get("result").map(|r| {
        let s = r.to_string();
        if s.len() > 200 { format!("{}...", &s[..200]) } else { s }
    }).unwrap_or_else(|| "null".to_string());

    if let Some(err) = v.get("error") {
        println!("[RPC] <- {} ERROR: {}", req.method, err);
        return Err(anyhow!("rpc error: {}", err));
    }

    println!("[RPC] <- {} result={}", req.method, result_str);
    Ok(v.get("result").cloned().unwrap_or(Value::Null))
}

fn handle_launcher_ipc(webview: &WebView, state: &AppState, req: &IpcRequest) -> Result<Value> {
    let devnet = state.devnet.as_ref().ok_or_else(|| anyhow!("Devnet not enabled"))?;
    match req.method.as_str() {
        "vibefi_listDapps" => {
            println!("launcher: fetching dapp list from logs");
            let dapps = list_dapps(devnet)?;
            Ok(serde_json::to_value(dapps)?)
        }
        "vibefi_launchDapp" => {
            let root_cid = req
                .params
                .get(0)
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("missing rootCid"))?;
            println!("launcher: fetch bundle {root_cid}");
            let bundle_dir = devnet.cache_dir.join(root_cid);
            ensure_bundle_cached(devnet, root_cid, &bundle_dir)?;
            println!("launcher: verify bundle manifest");
            verify_manifest(&bundle_dir)?;
            println!("launcher: verify CID via IPFS");
            let computed = compute_ipfs_cid(&bundle_dir, &devnet.ipfs_api)?;
            if computed != root_cid {
                return Err(anyhow!("CID mismatch: expected {root_cid} got {computed}"));
            }
            let dist_dir = bundle_dir.join(".vibefi").join("dist");
            if dist_dir.join("index.html").exists() {
                println!("launcher: using cached build");
            } else {
                println!("launcher: build bundle");
                build_bundle(&bundle_dir, &dist_dir)?;
            }
            {
                let mut current = state.current_bundle.lock().unwrap();
                *current = Some(dist_dir);
            }
            webview.evaluate_script("window.location = 'app://index.html';")?;
            Ok(Value::Bool(true))
        }
        _ => Err(anyhow!("Unsupported launcher method: {}", req.method)),
    }
}

fn ensure_bundle_cached(devnet: &DevnetContext, root_cid: &str, bundle_dir: &Path) -> Result<()> {
    if bundle_dir.join("manifest.json").exists() {
        return Ok(());
    }
    println!("launcher: download bundle from IPFS gateway");
    fs::create_dir_all(bundle_dir).context("create cache dir")?;
    let (manifest, manifest_bytes) = fetch_dapp_manifest(devnet, root_cid)?;
    download_dapp_bundle(devnet, root_cid, bundle_dir, &manifest, &manifest_bytes)?;
    Ok(())
}

fn fetch_dapp_manifest(devnet: &DevnetContext, root_cid: &str) -> Result<(BundleManifest, Vec<u8>)> {
    let gateway = normalize_gateway(&devnet.ipfs_gateway);
    let url = format!("{}/ipfs/{}/manifest.json", gateway, root_cid);
    let res = devnet.http.get(url).send().context("fetch manifest")?;
    if !res.status().is_success() {
        let text = res.text().unwrap_or_default();
        return Err(anyhow!("fetch manifest failed: {}", text));
    }
    let raw_bytes = res.bytes().context("read manifest bytes")?.to_vec();
    let manifest: BundleManifest = serde_json::from_slice(&raw_bytes).context("parse manifest")?;
    if manifest.files.is_empty() {
        return Err(anyhow!("manifest.json missing files list"));
    }
    Ok((manifest, raw_bytes))
}

fn download_dapp_bundle(
    devnet: &DevnetContext,
    root_cid: &str,
    out_dir: &Path,
    manifest: &BundleManifest,
    manifest_bytes: &[u8],
) -> Result<()> {
    let gateway = normalize_gateway(&devnet.ipfs_gateway);
    fs::write(out_dir.join("manifest.json"), manifest_bytes)?;
    for entry in &manifest.files {
        let url = format!("{}/ipfs/{}/{}", gateway, root_cid, entry.path);
        let res = devnet.http.get(url).send().context("fetch bundle file")?;
        if !res.status().is_success() {
            let text = res.text().unwrap_or_default();
            return Err(anyhow!("bundle fetch failed: {}", text));
        }
        let bytes = res.bytes().context("read bundle file")?;
        let dest = out_dir.join(&entry.path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(dest, &bytes)?;
    }
    Ok(())
}

fn compute_ipfs_cid(out_dir: &Path, ipfs_api: &str) -> Result<String> {
    let files = walk_files(out_dir)?;
    let mut form = Form::new();
    for file in files {
        let rel = file.strip_prefix(out_dir)?.to_string_lossy().replace('\\', "/");
        let data = fs::read(&file)?;
        let part = Part::bytes(data).file_name(rel);
        form = form.part("file", part);
    }
    let url = format!("{}/api/v0/add", ipfs_api.trim_end_matches('/'));
    let res = HttpClient::new()
        .post(url)
        .query(&[
            ("recursive", "true"),
            ("wrap-with-directory", "true"),
            ("cid-version", "1"),
            ("pin", "false"),
            ("only-hash", "true"),
        ])
        .multipart(form)
        .send()
        .context("ipfs add failed")?;
    let body = res.text().context("read ipfs response")?;
    let lines: Vec<&str> = body.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return Err(anyhow!("IPFS add returned empty response"));
    }
    let last = lines[lines.len() - 1];
    let json: Value = serde_json::from_str(last).context("parse ipfs response")?;
    if let Some(hash) = json.get("Hash").and_then(|v| v.as_str()) {
        return Ok(hash.to_string());
    }
    if let Some(hash) = json.get("Cid").and_then(|v| v.get("/")).and_then(|v| v.as_str()) {
        return Ok(hash.to_string());
    }
    Err(anyhow!("IPFS add response missing CID"))
}

fn walk_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // Skip generated build files (not part of bundle content)
        if name == "node_modules"
            || name == ".git"
            || name == ".vibefi"
            || name == "package.json"
            || name == "vite.config.ts"
            || name == "tsconfig.json"
            || name == "bun.lock"
            || name == "bun.lockb"
        {
            continue;
        }
        if entry.file_type()?.is_dir() {
            out.extend(walk_files(&path)?);
        } else if entry.file_type()?.is_file() {
            out.push(path);
        }
    }
    Ok(out)
}

fn normalize_gateway(gateway: &str) -> String {
    gateway.trim_end_matches('/').to_string()
}

fn parse_json<T: for<'de> Deserialize<'de>>(res: HttpResponse) -> Result<T> {
    let status = res.status();
    if !status.is_success() {
        let text = res.text().unwrap_or_default();
        return Err(anyhow!("HTTP {} {}", status, text));
    }
    Ok(res.json()?)
}

sol! {
    event DappPublished(uint256 indexed dappId, uint256 indexed versionId, bytes rootCid, address proposer);
    event DappUpgraded(
        uint256 indexed dappId,
        uint256 indexed fromVersionId,
        uint256 indexed toVersionId,
        bytes rootCid,
        address proposer
    );
    event DappMetadata(uint256 indexed dappId, uint256 indexed versionId, string name, string version, string description);
    event DappPaused(uint256 indexed dappId, uint256 indexed versionId, address pausedBy, string reason);
    event DappUnpaused(uint256 indexed dappId, uint256 indexed versionId, address unpausedBy, string reason);
    event DappDeprecated(uint256 indexed dappId, uint256 indexed versionId, address deprecatedBy, string reason);
}

#[derive(Debug, Deserialize)]
struct RpcLog {
    address: String,
    data: String,
    topics: Vec<String>,
    #[serde(default)]
    blockNumber: Option<String>,
    #[serde(default)]
    logIndex: Option<String>,
}

fn list_dapps(devnet: &DevnetContext) -> Result<Vec<DappInfo>> {
    if devnet.config.dappRegistry.is_empty() {
        return Err(anyhow!("devnet.json missing dappRegistry"));
    }
    let address = devnet.config.dappRegistry.clone();
    let published = rpc_get_logs(devnet, &address, DappPublished::SIGNATURE_HASH)?;
    let upgraded = rpc_get_logs(devnet, &address, DappUpgraded::SIGNATURE_HASH)?;
    let metadata = rpc_get_logs(devnet, &address, DappMetadata::SIGNATURE_HASH)?;
    let paused = rpc_get_logs(devnet, &address, DappPaused::SIGNATURE_HASH)?;
    let unpaused = rpc_get_logs(devnet, &address, DappUnpaused::SIGNATURE_HASH)?;
    let deprecated = rpc_get_logs(devnet, &address, DappDeprecated::SIGNATURE_HASH)?;

    let mut all = Vec::new();
    all.extend(published);
    all.extend(upgraded);
    all.extend(metadata);
    all.extend(paused);
    all.extend(unpaused);
    all.extend(deprecated);
    all.sort_by(|a, b| {
        let block_diff = a.block_number.cmp(&b.block_number);
        if block_diff != std::cmp::Ordering::Equal {
            return block_diff;
        }
        a.log_index.cmp(&b.log_index)
    });

    #[derive(Debug)]
    struct Version {
        version_id: u64,
        root_cid: Option<String>,
        name: Option<String>,
        version: Option<String>,
        description: Option<String>,
        status: Option<String>,
    }
    #[derive(Debug)]
    struct Dapp {
        dapp_id: u64,
        latest_version_id: u64,
        versions: HashMap<u64, Version>,
    }

    let mut dapps: HashMap<u64, Dapp> = HashMap::new();

    macro_rules! get_or_create_version {
        ($dapps:expr, $dapp_id:expr, $version_id:expr) => {{
            let dapp = $dapps.entry($dapp_id).or_insert_with(|| Dapp {
                dapp_id: $dapp_id,
                latest_version_id: 0,
                versions: HashMap::new(),
            });
            dapp.versions.entry($version_id).or_insert_with(|| Version {
                version_id: $version_id,
                root_cid: None,
                name: None,
                version: None,
                description: None,
                status: None,
            })
        }};
    }

    for log in all {
        match log.kind.as_str() {
            "DappPublished" => {
                let decoded = DappPublished::decode_log(&log.log)?;
                let dapp_id = u256_to_u64(decoded.data.dappId)?;
                let version_id = u256_to_u64(decoded.data.versionId)?;
                let root = bytes_to_string(&decoded.data.rootCid);
                let v = get_or_create_version!(dapps, dapp_id, version_id);
                v.root_cid = Some(root);
                v.status = Some("Published".to_string());
                dapps.get_mut(&dapp_id).unwrap().latest_version_id = version_id;
            }
            "DappUpgraded" => {
                let decoded = DappUpgraded::decode_log(&log.log)?;
                let dapp_id = u256_to_u64(decoded.data.dappId)?;
                let version_id = u256_to_u64(decoded.data.toVersionId)?;
                let root = bytes_to_string(&decoded.data.rootCid);
                let v = get_or_create_version!(dapps, dapp_id, version_id);
                v.root_cid = Some(root);
                v.status = Some("Published".to_string());
                dapps.get_mut(&dapp_id).unwrap().latest_version_id = version_id;
            }
            "DappMetadata" => {
                let decoded = DappMetadata::decode_log(&log.log)?;
                let dapp_id = u256_to_u64(decoded.data.dappId)?;
                let version_id = u256_to_u64(decoded.data.versionId)?;
                let v = get_or_create_version!(dapps, dapp_id, version_id);
                v.name = Some(decoded.data.name.to_string());
                v.version = Some(decoded.data.version.to_string());
                v.description = Some(decoded.data.description.to_string());
            }
            "DappPaused" => {
                let decoded = DappPaused::decode_log(&log.log)?;
                let dapp_id = u256_to_u64(decoded.data.dappId)?;
                let version_id = u256_to_u64(decoded.data.versionId)?;
                let v = get_or_create_version!(dapps, dapp_id, version_id);
                v.status = Some("Paused".to_string());
            }
            "DappUnpaused" => {
                let decoded = DappUnpaused::decode_log(&log.log)?;
                let dapp_id = u256_to_u64(decoded.data.dappId)?;
                let version_id = u256_to_u64(decoded.data.versionId)?;
                let v = get_or_create_version!(dapps, dapp_id, version_id);
                v.status = Some("Published".to_string());
            }
            "DappDeprecated" => {
                let decoded = DappDeprecated::decode_log(&log.log)?;
                let dapp_id = u256_to_u64(decoded.data.dappId)?;
                let version_id = u256_to_u64(decoded.data.versionId)?;
                let v = get_or_create_version!(dapps, dapp_id, version_id);
                v.status = Some("Deprecated".to_string());
            }
            _ => {}
        }
    }

    let mut result = Vec::new();
    let mut keys: Vec<u64> = dapps.keys().cloned().collect();
    keys.sort_unstable();
    for key in keys {
        if let Some(dapp) = dapps.get(&key) {
            let latest = dapp.versions.get(&dapp.latest_version_id);
            result.push(DappInfo {
                dappId: dapp.dapp_id.to_string(),
                versionId: dapp.latest_version_id.to_string(),
                name: latest.and_then(|v| v.name.clone()).unwrap_or_default(),
                version: latest.and_then(|v| v.version.clone()).unwrap_or_default(),
                description: latest.and_then(|v| v.description.clone()).unwrap_or_default(),
                status: latest.and_then(|v| v.status.clone()).unwrap_or_else(|| "Unknown".to_string()),
                rootCid: latest.and_then(|v| v.root_cid.clone()).unwrap_or_default(),
            });
        }
    }
    Ok(result)
}

struct LogEntry {
    block_number: u64,
    log_index: u64,
    kind: String,
    log: Log,
}

fn rpc_get_logs(devnet: &DevnetContext, address: &str, topic0: B256) -> Result<Vec<LogEntry>> {
    let topics = vec![format!("0x{}", hex::encode(topic0))];
    let from_block = devnet
        .config
        .deployBlock
        .map(|b| format!("0x{:x}", b))
        .unwrap_or_else(|| "0x0".to_string());
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_getLogs",
        "params": [{
            "address": address,
            "topics": topics,
            "fromBlock": from_block,
            "toBlock": "latest"
        }]
    });
    let res = devnet
        .http
        .post(&devnet.rpc_url)
        .json(&payload)
        .send()
        .context("rpc getLogs failed")?;
    let v: Value = res.json().context("rpc getLogs decode failed")?;
    if let Some(err) = v.get("error") {
        return Err(anyhow!("rpc getLogs error: {}", err));
    }
    let logs_val = v.get("result").cloned().unwrap_or(Value::Array(Vec::new()));
    let logs: Vec<RpcLog> = serde_json::from_value(logs_val)?;
    let mut out = Vec::new();
    for log in logs {
        let log_entry = rpc_log_to_entry(log, address)?;
        out.push(log_entry);
    }
    Ok(out)
}

fn rpc_log_to_entry(rpc_log: RpcLog, _address: &str) -> Result<LogEntry> {
    let address = Address::from_str(&rpc_log.address)?;
    let mut topics = Vec::new();
    for topic in rpc_log.topics {
        topics.push(hex_to_b256(&topic)?);
    }
    let data = hex_to_bytes(&rpc_log.data)?;
    let log = Log::new_unchecked(address, topics, data);
    let kind = event_kind(&log)?;
    Ok(LogEntry {
        block_number: parse_hex_u64_opt(rpc_log.blockNumber.as_deref()).unwrap_or(0),
        log_index: parse_hex_u64_opt(rpc_log.logIndex.as_deref()).unwrap_or(0),
        kind,
        log,
    })
}

fn event_kind(log: &Log) -> Result<String> {
    let topics = log.topics();
    if topics.is_empty() {
        return Err(anyhow!("log missing topics"));
    }
    let topic0 = topics[0];
    if topic0 == DappPublished::SIGNATURE_HASH {
        Ok("DappPublished".to_string())
    } else if topic0 == DappUpgraded::SIGNATURE_HASH {
        Ok("DappUpgraded".to_string())
    } else if topic0 == DappMetadata::SIGNATURE_HASH {
        Ok("DappMetadata".to_string())
    } else if topic0 == DappPaused::SIGNATURE_HASH {
        Ok("DappPaused".to_string())
    } else if topic0 == DappUnpaused::SIGNATURE_HASH {
        Ok("DappUnpaused".to_string())
    } else if topic0 == DappDeprecated::SIGNATURE_HASH {
        Ok("DappDeprecated".to_string())
    } else {
        Err(anyhow!("unknown event signature"))
    }
}

fn bytes_to_string(bytes: &Bytes) -> String {
    let mut out = bytes.to_vec();
    while out.last() == Some(&0) {
        out.pop();
    }
    String::from_utf8_lossy(&out).to_string()
}

fn hex_to_b256(s: &str) -> Result<B256> {
    let bytes = hex_to_vec(s)?;
    if bytes.len() != 32 {
        return Err(anyhow!("invalid topic length"));
    }
    Ok(B256::from_slice(&bytes))
}

fn hex_to_bytes(s: &str) -> Result<Bytes> {
    Ok(Bytes::from(hex_to_vec(s)?))
}

fn hex_to_vec(s: &str) -> Result<Vec<u8>> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    Ok(hex::decode(s)?)
}

fn parse_hex_u64_opt(s: Option<&str>) -> Option<u64> {
    s.and_then(|v| parse_hex_u64(v))
}

fn u256_to_u64(value: U256) -> Result<u64> {
    value.try_into().map_err(|_| anyhow!("u256 out of range"))
}

fn handle_ipc(webview: &WebView, state: &AppState, msg: String) -> Result<()> {
    let req: IpcRequest = serde_json::from_str(&msg).context("invalid IPC JSON")?;
    if matches!(req.provider_id.as_deref(), Some("vibefi-launcher")) {
        let result = handle_launcher_ipc(webview, state, &req);
        match result {
            Ok(v) => respond_ok(webview, req.id, v)?,
            Err(e) => respond_err(webview, req.id, &e.to_string())?,
        }
        return Ok(());
    }

    // Dispatch EIP-1193 methods.
    let result = match req.method.as_str() {
        // --- Basic identity ---
        "eth_chainId" => Ok(Value::String(state.chain_id_hex())),
        "net_version" => {
            let chain_id = state.wallet.lock().unwrap().chain.chain_id;
            Ok(Value::String(chain_id.to_string()))
        }

        // --- Accounts ---
        "eth_accounts" => {
            let ws = state.wallet.lock().unwrap();
            if ws.authorized {
                Ok(Value::Array(vec![Value::String(format!("0x{:x}", state.address()))]))
            } else {
                Ok(Value::Array(vec![]))
            }
        }
        "eth_requestAccounts" => {
            let mut ws = state.wallet.lock().unwrap();
            ws.authorized = true;
            drop(ws);

            let addr = state.address();
            emit_accounts_changed(webview, vec![addr]);

            Ok(Value::Array(vec![Value::String(format!("0x{:x}", addr))]))
        }

        // --- Chain switching (demo supports a small allowlist) ---
        "wallet_switchEthereumChain" => {
            // params: [{ chainId: "0x..." }]
            let chain_id_hex = req
                .params
                .get(0)
                .and_then(|v| v.get("chainId"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("invalid params for wallet_switchEthereumChain"))?;

            let chain_id = parse_hex_u64(chain_id_hex)
                .ok_or_else(|| anyhow!("invalid chainId"))?;

            // Demo allowlist: mainnet (1), sepolia (11155111), anvil (31337)
            if !matches!(chain_id, 1 | 11155111 | 31337) {
                return Err(anyhow!("Unsupported chainId in demo"));
            }

            {
                let mut ws = state.wallet.lock().unwrap();
                ws.chain.chain_id = chain_id;
            }

            let chain_hex = format!("0x{:x}", chain_id);
            emit_chain_changed(webview, chain_hex.clone());

            // EIP-1193: success -> null
            Ok(Value::Null)
        }

        // --- Signing (offline) ---
        // personal_sign: params [message, address]
        "personal_sign" => {
            let msg = req
                .params
                .get(0)
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("invalid params for personal_sign"))?;

            // Accept either raw string or 0x hex data.
            let bytes = if let Some(b) = decode_0x_hex(msg) {
                b
            } else {
                msg.as_bytes().to_vec()
            };

            let sig = state
                .signer
                .sign_message_sync(&bytes)
                .context("sign_message failed")?;

            Ok(Value::String(format!("0x{}", hex::encode(sig.as_bytes()))))
        }

        // eth_signTypedData_v4 (demo): params [address, jsonString]
        // We parse the JSON and sign the *hash* of it for demo purposes.
        // Proper EIP-712 hashing is more involved; Alloy can do it, but this example keeps it short.
        "eth_signTypedData_v4" => {
            let typed_data_json = req
                .params
                .get(1)
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("invalid params for eth_signTypedData_v4"))?;

            let hash = alloy_primitives::keccak256(typed_data_json.as_bytes());
            let sig = state
                .signer
                .sign_hash_sync(&B256::from(hash))
                .context("sign_hash failed")?;

            Ok(Value::String(format!("0x{}", hex::encode(sig.as_bytes()))))
        }

        // eth_sendTransaction: params [txObject]
        // If devnet is configured, proxy to anvil (which has accounts unlocked).
        // Otherwise use a demo signing mode that returns a fake tx hash.
        "eth_sendTransaction" => {
            let ws = state.wallet.lock().unwrap();
            if !ws.authorized {
                return Err(anyhow!("Unauthorized: call eth_requestAccounts first"));
            }
            drop(ws);

            // If devnet is configured, proxy to anvil for real transaction execution
            if state.devnet.is_some() {
                // Ensure from address is set to our wallet address
                let mut tx_obj = req
                    .params
                    .get(0)
                    .cloned()
                    .ok_or_else(|| anyhow!("invalid params for eth_sendTransaction"))?;

                // Set from address if not present
                if tx_obj.get("from").is_none() {
                    if let Some(obj) = tx_obj.as_object_mut() {
                        obj.insert("from".to_string(), Value::String(state.get_address()));
                    }
                }

                // Create modified request with updated params
                let modified_req = IpcRequest {
                    id: req.id,
                    provider_id: req.provider_id.clone(),
                    method: req.method.clone(),
                    params: Value::Array(vec![tx_obj]),
                };

                proxy_rpc(state, &modified_req)
            } else {
                // Fallback: demo mode - hash the tx and return fake hash (no network)
                let tx_obj = req
                    .params
                    .get(0)
                    .cloned()
                    .ok_or_else(|| anyhow!("invalid params for eth_sendTransaction"))?;

                let canonical = serde_json::to_vec(&tx_obj).context("tx json encode")?;
                let digest = alloy_primitives::keccak256(&canonical);
                let sig = state
                    .signer
                    .sign_hash_sync(&B256::from(digest))
                    .context("sign_hash failed")?;

                let tx_hash = alloy_primitives::keccak256(sig.as_bytes());
                Ok(Value::String(format!("0x{}", hex::encode(tx_hash))))
            }
        }

        // EIP-1193 provider info (non-standard but useful)
        "wallet_getProviderInfo" => {
            let info = ProviderInfo {
                name: "wry-demo-wallet",
                chain_id: state.chain_id_hex(),
            };
            Ok(serde_json::to_value(info)?)
        }

        _ => {
            if state.devnet.is_some() && is_rpc_passthrough(req.method.as_str()) {
                proxy_rpc(state, &req)
            } else {
                Err(anyhow!("Unsupported method: {}", req.method))
            }
        }
    };

    match result {
        Ok(v) => respond_ok(webview, req.id, v)?,
        Err(e) => respond_err(webview, req.id, &e.to_string())?,
    }

    Ok(())
}

fn respond_ok(webview: &WebView, id: u64, value: Value) -> Result<()> {
    let js = format!(
        "window.__WryEthereumResolve({}, {}, null);",
        id,
        value.to_string()
    );
    webview.evaluate_script(&js)?;
    Ok(())
}

fn respond_err(webview: &WebView, id: u64, message: &str) -> Result<()> {
    // EIP-1193 style error object
    let err = serde_json::json!({
        "code": -32601,
        "message": message,
    });
    let js = format!(
        "window.__WryEthereumResolve({}, null, {});",
        id,
        err.to_string()
    );
    webview.evaluate_script(&js)?;
    Ok(())
}

fn emit_accounts_changed(webview: &WebView, addrs: Vec<Address>) {
    let arr = addrs
        .into_iter()
        .map(|a| Value::String(format!("0x{:x}", a)))
        .collect::<Vec<_>>();
    let payload = Value::Array(arr);
    let js = format!("window.__WryEthereumEmit('accountsChanged', {});", payload);
    let _ = webview.evaluate_script(&js);
}

fn emit_chain_changed(webview: &WebView, chain_id_hex: String) {
    let payload = Value::String(chain_id_hex);
    let js = format!("window.__WryEthereumEmit('chainChanged', {});", payload);
    let _ = webview.evaluate_script(&js);
}

fn parse_hex_u64(s: &str) -> Option<u64> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    u64::from_str_radix(s, 16).ok()
}

fn decode_0x_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.strip_prefix("0x")?;
    if s.len() % 2 != 0 {
        return None;
    }
    hex::decode(s).ok()
}

#[cfg(target_os = "macos")]
fn setup_macos_app_menu(app_name: &str) {
    use objc2::{sel, MainThreadOnly};
    use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
    use objc2_foundation::{MainThreadMarker, NSString};

    let mtm = MainThreadMarker::new().unwrap();
    let app = NSApplication::sharedApplication(mtm);
    if app.mainMenu().is_some() {
        return;
    }

    let menubar = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(""));

    let app_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(app_name));
    let app_menu_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(""),
            None,
            &NSString::from_str(""),
        )
    };
    menubar.addItem(&app_menu_item);
    app_menu_item.setSubmenu(Some(&app_menu));

    let quit_title = format!("Quit {app_name}");
    let quit_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(&quit_title),
            Some(sel!(terminate:)),
            &NSString::from_str("q"),
        )
    };
    quit_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    app_menu.addItem(&quit_item);

    let edit_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Edit"));
    let edit_menu_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Edit"),
            None,
            &NSString::from_str(""),
        )
    };
    menubar.addItem(&edit_menu_item);
    edit_menu_item.setSubmenu(Some(&edit_menu));

    let undo_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Undo"),
            Some(sel!(undo:)),
            &NSString::from_str("z"),
        )
    };
    undo_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    edit_menu.addItem(&undo_item);

    let redo_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Redo"),
            Some(sel!(redo:)),
            &NSString::from_str("Z"),
        )
    };
    redo_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command | NSEventModifierFlags::Shift);
    edit_menu.addItem(&redo_item);

    edit_menu.addItem(&NSMenuItem::separatorItem(mtm));

    let cut_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Cut"),
            Some(sel!(cut:)),
            &NSString::from_str("x"),
        )
    };
    cut_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    edit_menu.addItem(&cut_item);

    let copy_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Copy"),
            Some(sel!(copy:)),
            &NSString::from_str("c"),
        )
    };
    copy_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    edit_menu.addItem(&copy_item);

    let paste_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Paste"),
            Some(sel!(paste:)),
            &NSString::from_str("v"),
        )
    };
    paste_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    edit_menu.addItem(&paste_item);

    let select_all_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Select All"),
            Some(sel!(selectAll:)),
            &NSString::from_str("a"),
        )
    };
    select_all_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    edit_menu.addItem(&select_all_item);

    app.setMainMenu(Some(&menubar));
}
