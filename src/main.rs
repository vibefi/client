use anyhow::{anyhow, Context, Result};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tao::{
    event::{Event, StartCause, WindowEvent},
    event_loop::ControlFlow,
    window::WindowBuilder,
};
use wry::{
    http::{header::CONTENT_TYPE, Response},
    WebView, WebViewBuilder,
};

use alloy_primitives::{Address, B256};
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;

static INDEX_HTML: &str = include_str!("../assets/index.html");

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
struct IpcRequest {
    id: u64,
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
}

impl AppState {
    fn address(&self) -> Address {
        self.signer.address()
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

fn main() -> Result<()> {
    // --- Build signing wallet (Alloy) ---
    let signer: PrivateKeySigner = DEMO_PRIVKEY_HEX
        .parse()
        .context("failed to parse demo private key")?;

    let state = AppState {
        wallet: Arc::new(Mutex::new(WalletState::default())),
        signer: Arc::new(signer),
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
                        .with_title("Wry EIP-1193 demo (no network)")
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

                    let built = build_webview(&window_handle, state.clone(), proxy.clone());
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
) -> Result<WebView> {
    let wallet_state = state.wallet.clone();

    // Serve only our embedded assets.
    let protocol = move |_webview_id: wry::WebViewId, request: wry::http::Request<Vec<u8>>| {
        let path = request.uri().path();
        let (body, mime) = match path {
            "/" | "/index.html" => (INDEX_HTML.as_bytes().to_vec(), "text/html; charset=utf-8"),
            _ => (format!("Not found: {path}").into_bytes(), "text/plain; charset=utf-8"),
        };

        Response::builder()
            .status(200)
            .header(CONTENT_TYPE, mime)
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

fn handle_ipc(webview: &WebView, state: &AppState, msg: String) -> Result<()> {
    let req: IpcRequest = serde_json::from_str(&msg).context("invalid IPC JSON")?;

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
        // Demo signs a "transaction-like" digest and returns a fake tx hash (no network).
        "eth_sendTransaction" => {
            let ws = state.wallet.lock().unwrap();
            if !ws.authorized {
                return Err(anyhow!("Unauthorized: call eth_requestAccounts first"));
            }
            drop(ws);

            let tx_obj = req
                .params
                .get(0)
                .cloned()
                .ok_or_else(|| anyhow!("invalid params for eth_sendTransaction"))?;

            // Hash the canonical JSON as a stand-in.
            let canonical = serde_json::to_vec(&tx_obj).context("tx json encode")?;
            let digest = alloy_primitives::keccak256(&canonical);
            let sig = state
                .signer
                .sign_hash_sync(&B256::from(digest))
                .context("sign_hash failed")?;

            // Produce a stable "tx hash" = keccak256(signature bytes)
            let tx_hash = alloy_primitives::keccak256(sig.as_bytes());
            Ok(Value::String(format!("0x{}", hex::encode(tx_hash))))
        }

        // EIP-1193 provider info (non-standard but useful)
        "wallet_getProviderInfo" => {
            let info = ProviderInfo {
                name: "wry-demo-wallet",
                chain_id: state.chain_id_hex(),
            };
            Ok(serde_json::to_value(info)?)
        }

        _ => Err(anyhow!("Unsupported method: {}", req.method)),
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
