use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use std::fs;
use wry::{
    http::{header::CONTENT_TYPE, Response},
    WebView, WebViewBuilder,
};

use crate::bundle::BundleConfig;
use crate::ipc::{emit_accounts_changed, emit_chain_changed};
use crate::state::{AppState, UserEvent};
use crate::{INDEX_HTML, LAUNCHER_HTML};

pub static INIT_SCRIPT: Lazy<String> = Lazy::new(|| {
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

pub fn build_webview(
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
                let guess = mime_guess::MimeGuess::from_path(&file_path)
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
