use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use wry::{
    Rect, WebView, WebViewBuilder,
    http::{Response, header::CONTENT_TYPE},
};

use crate::ipc::{emit_accounts_changed, emit_chain_changed};
use crate::state::{AppState, UserEvent};
use crate::{
    HOME_JS, INDEX_HTML, LAUNCHER_HTML, LAUNCHER_JS, PRELOAD_APP_JS, PRELOAD_SETTINGS_JS,
    PRELOAD_TAB_BAR_JS, PRELOAD_WALLET_SELECTOR_JS, SETTINGS_HTML, SETTINGS_JS, TAB_BAR_HTML,
    TAB_BAR_JS, WALLET_SELECTOR_HTML, WALLET_SELECTOR_JS,
};

/// What embedded content to serve when `dist_dir` is `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddedContent {
    /// The default demo `index.html`.
    Default,
    /// The devnet launcher (launcher.html + launcher.js).
    Launcher,
    /// The runtime wallet-selector tab.
    WalletSelector,
    /// The settings tab.
    Settings,
}

fn serve_file(dist_dir: &PathBuf, path: &str) -> (Vec<u8>, String) {
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
}

fn normalized_app_path(uri: &wry::http::Uri) -> String {
    let mut path = uri.path().to_string();
    if (path.is_empty() || path == "/") && uri.host().is_some() {
        if let Some(host) = uri.host() {
            path = format!("/{}", host);
        }
    }

    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", trimmed)
    }
}

fn csp_response(
    body: Vec<u8>,
    mime: String,
) -> wry::http::Response<std::borrow::Cow<'static, [u8]>> {
    Response::builder()
        .status(200)
        .header(CONTENT_TYPE, mime.as_str())
        .header(
            "Content-Security-Policy",
            "default-src 'self' app:; img-src 'self' data: app:; style-src 'self' 'unsafe-inline' app:; script-src 'self' 'unsafe-inline' app:; connect-src 'none'; frame-src 'none'",
        )
        .body(std::borrow::Cow::Owned(body))
        .unwrap()
}

pub fn build_app_webview(
    window: &tao::window::Window,
    id: &str,
    dist_dir: Option<PathBuf>,
    embedded: EmbeddedContent,
    state: &AppState,
    proxy: tao::event_loop::EventLoopProxy<UserEvent>,
    bounds: Rect,
) -> Result<WebView> {
    let protocol_dist = dist_dir.clone();
    let protocol = move |_webview_id: wry::WebViewId, request: wry::http::Request<Vec<u8>>| {
        let path = normalized_app_path(request.uri());
        if let Some(ref dist) = protocol_dist {
            let (body, mime) = serve_file(dist, &path);
            csp_response(body, mime)
        } else {
            match (embedded, path.as_str()) {
                (_, "/" | "/index.html") => {
                    let html = match embedded {
                        EmbeddedContent::Default => INDEX_HTML,
                        EmbeddedContent::Launcher => LAUNCHER_HTML,
                        EmbeddedContent::WalletSelector => WALLET_SELECTOR_HTML,
                        EmbeddedContent::Settings => SETTINGS_HTML,
                    };
                    csp_response(
                        html.as_bytes().to_vec(),
                        "text/html; charset=utf-8".to_string(),
                    )
                }
                (EmbeddedContent::Launcher, "/launcher.js") => csp_response(
                    LAUNCHER_JS.as_bytes().to_vec(),
                    "application/javascript; charset=utf-8".to_string(),
                ),
                (EmbeddedContent::Default, "/home.js") => csp_response(
                    HOME_JS.as_bytes().to_vec(),
                    "application/javascript; charset=utf-8".to_string(),
                ),
                (EmbeddedContent::WalletSelector, "/wallet-selector.js") => csp_response(
                    WALLET_SELECTOR_JS.as_bytes().to_vec(),
                    "application/javascript; charset=utf-8".to_string(),
                ),
                (EmbeddedContent::Settings, "/settings.js") => csp_response(
                    SETTINGS_JS.as_bytes().to_vec(),
                    "application/javascript; charset=utf-8".to_string(),
                ),
                _ => csp_response(
                    format!("Not found: {}", path).into_bytes(),
                    "text/plain; charset=utf-8".to_string(),
                ),
            }
        }
    };

    let navigation_handler = |url: String| url.starts_with("app://") || url == "about:blank";

    let init_script = match embedded {
        EmbeddedContent::WalletSelector => PRELOAD_WALLET_SELECTOR_JS.to_string(),
        EmbeddedContent::Settings => PRELOAD_SETTINGS_JS.to_string(),
        _ => PRELOAD_APP_JS.to_string(),
    };

    let webview_id = id.to_string();
    let webview = WebViewBuilder::new()
        .with_id(id)
        .with_bounds(bounds)
        .with_initialization_script(init_script)
        .with_custom_protocol("app".into(), protocol)
        .with_url("app://index.html")
        .with_navigation_handler(navigation_handler)
        .with_ipc_handler(move |req: wry::http::Request<String>| {
            let _ = proxy.send_event(UserEvent::Ipc {
                webview_id: webview_id.clone(),
                msg: req.body().clone(),
            });
        })
        .build_as_child(window)
        .context("failed to build app webview")?;

    // Emit initial chain/accounts state after load (skip for selector and settings tabs).
    if embedded != EmbeddedContent::WalletSelector && embedded != EmbeddedContent::Settings {
        let addr = state.account();
        let chain_hex = state.chain_id_hex();
        {
            let ws = state.wallet.lock().unwrap();
            if ws.authorized {
                if let Some(addr) = addr {
                    emit_accounts_changed(&webview, vec![addr]);
                }
            }
        }
        emit_chain_changed(&webview, chain_hex);
    }

    Ok(webview)
}

pub fn build_tab_bar_webview(
    window: &tao::window::Window,
    proxy: tao::event_loop::EventLoopProxy<UserEvent>,
    bounds: Rect,
) -> Result<WebView> {
    let protocol = move |_webview_id: wry::WebViewId, request: wry::http::Request<Vec<u8>>| {
        let path = normalized_app_path(request.uri());
        let (body, mime) = match path.as_str() {
            "/" | "/index.html" | "/tabbar.html" => (
                TAB_BAR_HTML.as_bytes().to_vec(),
                "text/html; charset=utf-8".to_string(),
            ),
            "/tabbar.js" => (
                TAB_BAR_JS.as_bytes().to_vec(),
                "application/javascript; charset=utf-8".to_string(),
            ),
            _ => (
                format!("Not found: {}", path).into_bytes(),
                "text/plain; charset=utf-8".to_string(),
            ),
        };
        csp_response(body, mime)
    };

    let webview = WebViewBuilder::new()
        .with_id("tab-bar")
        .with_bounds(bounds)
        .with_initialization_script(PRELOAD_TAB_BAR_JS.to_string())
        .with_custom_protocol("app".into(), protocol)
        .with_url("app://tabbar.html")
        .with_ipc_handler(move |req: wry::http::Request<String>| {
            let _ = proxy.send_event(UserEvent::Ipc {
                webview_id: "tab-bar".to_string(),
                msg: req.body().clone(),
            });
        })
        .build_as_child(window)
        .context("failed to build tab bar webview")?;

    Ok(webview)
}
