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
  const PROVIDER_ID = 'vibefi-provider';
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
    // Built-in WalletConnect pairing UX:
    // show a lightweight overlay as soon as Rust forwards a `walletconnect_uri` message.
    const first = args[0];
    if (event === 'message' && first && first.type === 'walletconnect_uri' && typeof first.data === 'string') {
      showWalletConnectOverlay(first.data);
    }
    if (event === 'accountsChanged' && Array.isArray(first) && first.length > 0) {
      hideWalletConnectOverlay();
    }

    const set = listeners.get(event);
    if (!set) return;
    for (const h of Array.from(set)) {
      try { h(...args); } catch (_) {}
    }
  }

  let wcOverlay = null;
  let wcUriEl = null;
  function ensureWalletConnectOverlay() {
    if (wcOverlay) return;
    const panel = document.createElement('div');
    panel.style.position = 'fixed';
    panel.style.right = '12px';
    panel.style.bottom = '12px';
    panel.style.width = 'min(560px, calc(100vw - 24px))';
    panel.style.background = 'rgba(2, 6, 23, 0.96)';
    panel.style.color = '#e2e8f0';
    panel.style.border = '1px solid rgba(148, 163, 184, 0.35)';
    panel.style.borderRadius = '12px';
    panel.style.padding = '12px';
    panel.style.fontSize = '12px';
    panel.style.lineHeight = '1.4';
    panel.style.zIndex = '2147483647';
    panel.style.boxShadow = '0 20px 40px rgba(0, 0, 0, 0.4)';
    panel.style.display = 'none';
    panel.innerHTML = `
      <div style="display:flex;justify-content:space-between;align-items:center;gap:8px;margin-bottom:8px;">
        <strong>WalletConnect Pairing</strong>
        <button id="__vibefi_wc_close" style="border:1px solid #475569;background:#0f172a;color:#e2e8f0;border-radius:8px;padding:4px 8px;cursor:pointer;">Hide</button>
      </div>
      <div style="opacity:0.9;margin-bottom:8px;">Open a WalletConnect-compatible wallet and approve the session. You can copy the pairing URI below.</div>
      <textarea id="__vibefi_wc_uri" readonly style="width:100%;height:92px;background:#020617;color:#93c5fd;border:1px solid #1e293b;border-radius:8px;padding:8px;resize:vertical;font-family:ui-monospace, Menlo, Monaco, Consolas, monospace;"></textarea>
      <div style="display:flex;justify-content:flex-end;margin-top:8px;">
        <button id="__vibefi_wc_copy" style="border:1px solid #475569;background:#0f172a;color:#e2e8f0;border-radius:8px;padding:6px 10px;cursor:pointer;">Copy URI</button>
      </div>
    `;
    document.body.appendChild(panel);

    wcOverlay = panel;
    wcUriEl = panel.querySelector('#__vibefi_wc_uri');
    const closeBtn = panel.querySelector('#__vibefi_wc_close');
    const copyBtn = panel.querySelector('#__vibefi_wc_copy');
    closeBtn?.addEventListener('click', () => hideWalletConnectOverlay());
    copyBtn?.addEventListener('click', async () => {
      const value = wcUriEl?.value ?? '';
      if (!value) return;
      try {
        if (navigator.clipboard?.writeText) {
          await navigator.clipboard.writeText(value);
          return;
        }
      } catch (_) {}
      try {
        wcUriEl?.focus();
        wcUriEl?.select();
        document.execCommand('copy');
      } catch (_) {}
    });
  }

  function showWalletConnectOverlay(uri) {
    ensureWalletConnectOverlay();
    if (wcUriEl) wcUriEl.value = uri;
    if (wcOverlay) wcOverlay.style.display = 'block';
  }

  function hideWalletConnectOverlay() {
    if (wcOverlay) wcOverlay.style.display = 'none';
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
        let active_bundle = current_bundle
            .lock()
            .unwrap()
            .clone()
            .or_else(|| protocol_bundle.as_ref().map(|cfg| cfg.dist_dir.clone()));
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
                        (
                            LAUNCHER_HTML.as_bytes().to_vec(),
                            "text/html; charset=utf-8".to_string(),
                        )
                    } else {
                        (
                            INDEX_HTML.as_bytes().to_vec(),
                            "text/html; charset=utf-8".to_string(),
                        )
                    }
                }
                _ => (
                    format!("Not found: {path}").into_bytes(),
                    "text/plain; charset=utf-8".to_string(),
                ),
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
    let addr = state.account();
    let chain_hex = state.chain_id_hex();
    {
        let ws = wallet_state.lock().unwrap();
        if ws.authorized {
            if let Some(addr) = addr {
                emit_accounts_changed(&webview, vec![addr]);
            }
        }
    }
    emit_chain_changed(&webview, chain_hex);

    Ok(webview)
}
