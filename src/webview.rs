use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use wry::WebViewBuilderExtUnix;
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

/// Platform-aware container for building child webviews.
/// On Linux (Wayland), `build_as_child` is unsupported; we use `build_gtk` with
/// `gtk::Box` containers that GTK lays out natively (avoiding CSD offset issues
/// with `gtk::Fixed` + manual `set_bounds`).
pub struct WebViewHost<'a> {
    pub window: &'a tao::window::Window,
    #[cfg(target_os = "linux")]
    pub tab_bar_container: &'a gtk::Box,
    #[cfg(target_os = "linux")]
    pub app_container: &'a gtk::Box,
}

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
    eprintln!(
        "[webview:debug] normalized_app_path: raw uri={uri}, scheme={:?}, host={:?}, path={:?}",
        uri.scheme_str(),
        uri.host(),
        uri.path()
    );
    let mut path = uri.path().to_string();
    if (path.is_empty() || path == "/") && uri.host().is_some() {
        if let Some(host) = uri.host() {
            path = format!("/{}", host);
        }
    }

    let trimmed = path.trim_start_matches('/');
    let result = if trimmed.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", trimmed)
    };
    eprintln!("[webview:debug] normalized_app_path: result={result:?}");
    result
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
            "default-src 'self' app: https://app.localhost; img-src 'self' data: app: https://app.localhost; style-src 'self' 'unsafe-inline' app: https://app.localhost; script-src 'self' 'unsafe-inline' app: https://app.localhost; connect-src 'none'; frame-src 'none'",
        )
        .body(std::borrow::Cow::Owned(body))
        .unwrap()
}

fn should_enable_devtools() -> bool {
    if cfg!(debug_assertions) {
        return true;
    }

    std::env::var("VIBEFI_ENABLE_DEVTOOLS")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn allow_navigation(url: &str) -> bool {
    if url == "about:blank" {
        return true;
    }

    let Ok(uri) = url.parse::<wry::http::Uri>() else {
        return false;
    };

    match uri.scheme_str() {
        Some("app") => true,
        Some("https") | Some("http") => {
            let host = uri.host().unwrap_or("");
            // wry rewrites custom protocol app://X to http://app.X/
            // e.g. app://index.html -> http://app.index.html/
            // Allow app.localhost and any app.* that ends in .html (rewritten filenames)
            let allowed_host = host == "app.localhost"
                || (host.starts_with("app.") && host.ends_with(".html"));
            allowed_host && uri.port().is_none()
        }
        _ => false,
    }
}

pub fn build_app_webview(
    host: &WebViewHost,
    id: &str,
    dist_dir: Option<PathBuf>,
    embedded: EmbeddedContent,
    state: &AppState,
    proxy: tao::event_loop::EventLoopProxy<UserEvent>,
    bounds: Rect,
) -> Result<WebView> {
    eprintln!(
        "[webview:debug] build_app_webview: id={id:?}, embedded={embedded:?}, dist_dir={dist_dir:?}, bounds={bounds:?}"
    );

    let protocol_dist = dist_dir.clone();
    let app_id_for_log = id.to_string();
    let protocol = move |_webview_id: wry::WebViewId, request: wry::http::Request<Vec<u8>>| {
        eprintln!(
            "[webview:debug] app protocol handler ({app_id_for_log}): method={} uri={}",
            request.method(),
            request.uri()
        );
        let path = normalized_app_path(request.uri());
        if let Some(ref dist) = protocol_dist {
            eprintln!("[webview:debug] serving from dist_dir: path={path:?}");
            let (body, mime) = serve_file(dist, &path);
            eprintln!(
                "[webview:debug] dist response: mime={mime:?}, body_len={}",
                body.len()
            );
            csp_response(body, mime)
        } else {
            let matched = match (embedded, path.as_str()) {
                (_, "/" | "/index.html") => {
                    let html = match embedded {
                        EmbeddedContent::Default => INDEX_HTML,
                        EmbeddedContent::Launcher => LAUNCHER_HTML,
                        EmbeddedContent::WalletSelector => WALLET_SELECTOR_HTML,
                        EmbeddedContent::Settings => SETTINGS_HTML,
                    };
                    eprintln!(
                        "[webview:debug] serving embedded html for {embedded:?}, len={}",
                        html.len()
                    );
                    csp_response(
                        html.as_bytes().to_vec(),
                        "text/html; charset=utf-8".to_string(),
                    )
                }
                (EmbeddedContent::Launcher, "/launcher.js") => {
                    eprintln!(
                        "[webview:debug] serving embedded launcher.js, len={}",
                        LAUNCHER_JS.len()
                    );
                    csp_response(
                        LAUNCHER_JS.as_bytes().to_vec(),
                        "application/javascript; charset=utf-8".to_string(),
                    )
                }
                (EmbeddedContent::Default, "/home.js") => {
                    eprintln!(
                        "[webview:debug] serving embedded home.js, len={}",
                        HOME_JS.len()
                    );
                    csp_response(
                        HOME_JS.as_bytes().to_vec(),
                        "application/javascript; charset=utf-8".to_string(),
                    )
                }
                (EmbeddedContent::WalletSelector, "/wallet-selector.js") => csp_response(
                    WALLET_SELECTOR_JS.as_bytes().to_vec(),
                    "application/javascript; charset=utf-8".to_string(),
                ),
                (EmbeddedContent::Settings, "/settings.js") => csp_response(
                    SETTINGS_JS.as_bytes().to_vec(),
                    "application/javascript; charset=utf-8".to_string(),
                ),
                _ => {
                    eprintln!("[webview:debug] NOT FOUND: embedded={embedded:?}, path={path:?}");
                    csp_response(
                        format!("Not found: {}", path).into_bytes(),
                        "text/plain; charset=utf-8".to_string(),
                    )
                }
            };
            matched
        }
    };

    let navigation_handler = |url: String| {
        let allowed = allow_navigation(&url);
        eprintln!("[webview:debug] navigation_handler: url={url:?} -> allowed={allowed}");
        allowed
    };

    let init_script = match embedded {
        EmbeddedContent::WalletSelector => PRELOAD_WALLET_SELECTOR_JS.to_string(),
        EmbeddedContent::Settings => PRELOAD_SETTINGS_JS.to_string(),
        _ => PRELOAD_APP_JS.to_string(),
    };

    let webview_id = id.to_string();
    let builder = WebViewBuilder::new()
        .with_id(id)
        .with_bounds(bounds)
        .with_initialization_script(init_script)
        .with_devtools(should_enable_devtools())
        .with_custom_protocol("app".into(), protocol)
        .with_url("app://index.html")
        .with_navigation_handler(navigation_handler)
        .with_ipc_handler(move |req: wry::http::Request<String>| {
            let _ = proxy.send_event(UserEvent::Ipc {
                webview_id: webview_id.clone(),
                msg: req.body().clone(),
            });
        });

    eprintln!("[webview:debug] building app webview (id={id})...");
    #[cfg(target_os = "linux")]
    let webview = builder
        .build_gtk(host.app_container)
        .context("failed to build app webview")?;
    #[cfg(not(target_os = "linux"))]
    let webview = builder
        .build_as_child(host.window)
        .context("failed to build app webview")?;
    eprintln!("[webview:debug] app webview built successfully (id={id})");

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
    host: &WebViewHost,
    proxy: tao::event_loop::EventLoopProxy<UserEvent>,
    bounds: Rect,
) -> Result<WebView> {
    eprintln!("[webview:debug] build_tab_bar_webview: bounds={bounds:?}");

    let protocol = move |_webview_id: wry::WebViewId, request: wry::http::Request<Vec<u8>>| {
        eprintln!(
            "[webview:debug] tabbar protocol handler: method={} uri={}",
            request.method(),
            request.uri()
        );
        let path = normalized_app_path(request.uri());
        let (body, mime) = match path.as_str() {
            "/" | "/index.html" | "/tabbar.html" => {
                eprintln!(
                    "[webview:debug] tabbar: serving tabbar.html, len={}",
                    TAB_BAR_HTML.len()
                );
                (
                    TAB_BAR_HTML.as_bytes().to_vec(),
                    "text/html; charset=utf-8".to_string(),
                )
            }
            "/tabbar.js" => {
                eprintln!(
                    "[webview:debug] tabbar: serving tabbar.js, len={}",
                    TAB_BAR_JS.len()
                );
                (
                    TAB_BAR_JS.as_bytes().to_vec(),
                    "application/javascript; charset=utf-8".to_string(),
                )
            }
            _ => {
                eprintln!("[webview:debug] tabbar: NOT FOUND path={path:?}");
                (
                    format!("Not found: {}", path).into_bytes(),
                    "text/plain; charset=utf-8".to_string(),
                )
            }
        };
        csp_response(body, mime)
    };

    let builder = WebViewBuilder::new()
        .with_id("tab-bar")
        .with_bounds(bounds)
        .with_initialization_script(PRELOAD_TAB_BAR_JS.to_string())
        .with_devtools(should_enable_devtools())
        .with_custom_protocol("app".into(), protocol)
        .with_url("app://tabbar.html")
        .with_ipc_handler(move |req: wry::http::Request<String>| {
            let _ = proxy.send_event(UserEvent::Ipc {
                webview_id: "tab-bar".to_string(),
                msg: req.body().clone(),
            });
        });

    eprintln!("[webview:debug] building tab bar webview...");
    #[cfg(target_os = "linux")]
    let webview = builder
        .build_gtk(host.tab_bar_container)
        .context("failed to build tab bar webview")?;
    #[cfg(not(target_os = "linux"))]
    let webview = builder
        .build_as_child(host.window)
        .context("failed to build tab bar webview")?;
    eprintln!("[webview:debug] tab bar webview built successfully");

    Ok(webview)
}

#[cfg(test)]
mod tests {
    use super::allow_navigation;

    #[test]
    fn allows_internal_navigation_origins() {
        assert!(allow_navigation("app://index.html"));
        assert!(allow_navigation("https://app.localhost/index.html"));
        assert!(allow_navigation("http://app.localhost/tabbar.html"));
        // wry rewrites app://index.html to http://app.index.html/
        assert!(allow_navigation("http://app.index.html/"));
        assert!(allow_navigation("http://app.tabbar.html/"));
        assert!(allow_navigation("about:blank"));
    }

    #[test]
    fn rejects_external_or_similar_lookalike_origins() {
        assert!(!allow_navigation("https://app.localhost.attacker.tld/index.html"));
        assert!(!allow_navigation("https://evil.tld"));
        assert!(!allow_navigation("https://app.localhost:8443/index.html"));
        assert!(!allow_navigation("not-a-url"));
    }
}
