use std::path::PathBuf;
use wry::{dpi::PhysicalPosition, dpi::PhysicalSize, Rect, WebView};

/// On macOS, bring a child webview to the front of the window's view hierarchy.
/// Walk up from the WKWebView until we find a view whose superview is the
/// window's contentView, then remove+re-add that view so it becomes the
/// topmost subview.
#[cfg(target_os = "macos")]
fn bring_webview_to_front(webview: &WebView) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use wry::WebViewExtMacOS;

    let ns_window = webview.ns_window();
    let wv = webview.webview();
    unsafe {
        let content_view: *mut AnyObject = msg_send![&*ns_window, contentView];
        if content_view.is_null() {
            return;
        }
        // Walk up from the WKWebView to find the direct child of contentView.
        let mut view: *mut AnyObject = &*wv as *const _ as *mut AnyObject;
        loop {
            let sup: *mut AnyObject = msg_send![view, superview];
            if sup.is_null() {
                return;
            }
            if sup == content_view {
                // `view` is a direct child of contentView — reorder it to front.
                let _: () = msg_send![view, removeFromSuperview];
                let _: () = msg_send![content_view, addSubview: view];
                return;
            }
            view = sup;
        }
    }
}

/// Logical tab bar height in points. Must be scaled by the window's scale factor
/// to get the physical pixel height used in `Rect` bounds.
pub const TAB_BAR_HEIGHT_LOGICAL: f64 = 40.0;

pub struct AppWebViewEntry {
    pub webview: WebView,
    pub id: String,
    pub label: String,
    pub dist_dir: Option<PathBuf>,
}

pub struct WebViewManager {
    pub tab_bar: Option<WebView>,
    pub wallet: Option<WebView>,
    pub apps: Vec<AppWebViewEntry>,
    pub active_app_index: Option<usize>,
    next_id: u64,
    scale_factor: f64,
}

impl WebViewManager {
    pub fn new(scale_factor: f64) -> Self {
        Self {
            tab_bar: None,
            wallet: None,
            apps: Vec::new(),
            active_app_index: None,
            next_id: 0,
            scale_factor,
        }
    }

    pub fn set_scale_factor(&mut self, scale_factor: f64) {
        self.scale_factor = scale_factor;
    }

    fn tab_bar_height_px(&self) -> u32 {
        (TAB_BAR_HEIGHT_LOGICAL * self.scale_factor) as u32
    }

    pub fn next_app_id(&mut self) -> String {
        let id = format!("app-{}", self.next_id);
        self.next_id += 1;
        id
    }

    pub fn webview_for_id(&self, id: &str) -> Option<&WebView> {
        if id == "tab-bar" {
            return self.tab_bar.as_ref();
        }
        if id == "wallet" {
            return self.wallet.as_ref();
        }
        self.apps.iter().find(|e| e.id == id).map(|e| &e.webview)
    }

    pub fn active_app_webview(&self) -> Option<&WebView> {
        self.active_app_index
            .and_then(|i| self.apps.get(i))
            .map(|e| &e.webview)
    }

    pub fn switch_to(&mut self, index: usize) {
        if index >= self.apps.len() {
            return;
        }
        if let Some(old) = self.active_app_index {
            if old < self.apps.len() {
                let _ = self.apps[old].webview.set_visible(false);
            }
        }
        let _ = self.apps[index].webview.set_visible(true);
        self.active_app_index = Some(index);
        self.update_tab_bar();
    }

    pub fn close_app(&mut self, index: usize) {
        if index >= self.apps.len() {
            return;
        }
        // Don't close the last tab (launcher)
        if self.apps.len() <= 1 {
            return;
        }
        self.apps.remove(index);
        // Adjust active index
        let new_active = if self.apps.is_empty() {
            None
        } else if let Some(old) = self.active_app_index {
            if old == index {
                Some(index.min(self.apps.len() - 1))
            } else if old > index {
                Some(old - 1)
            } else {
                Some(old)
            }
        } else {
            Some(0)
        };
        self.active_app_index = new_active;
        if let Some(i) = new_active {
            let _ = self.apps[i].webview.set_visible(true);
        }
        self.update_tab_bar();
    }

    pub fn relayout(&self, phys_width: u32, phys_height: u32) {
        let tb_h = self.tab_bar_height_px();
        let tab_rect = Rect {
            position: PhysicalPosition::new(0, 0).into(),
            size: PhysicalSize::new(phys_width, tb_h).into(),
        };
        if let Some(tb) = &self.tab_bar {
            let _ = tb.set_bounds(tab_rect);
        }

        let app_height = phys_height.saturating_sub(tb_h);
        let app_rect = Rect {
            position: PhysicalPosition::new(0i32, tb_h as i32).into(),
            size: PhysicalSize::new(phys_width, app_height).into(),
        };
        for entry in &self.apps {
            let _ = entry.webview.set_bounds(app_rect);
        }
    }

    pub fn broadcast_to_apps(&self, js: &str) {
        for entry in &self.apps {
            let _ = entry.webview.evaluate_script(js);
        }
    }

    pub fn update_tab_bar(&self) {
        let tb = match &self.tab_bar {
            Some(tb) => tb,
            None => return,
        };
        let tabs: Vec<serde_json::Value> = self
            .apps
            .iter()
            .map(|e| serde_json::json!({ "id": e.id, "label": e.label }))
            .collect();
        let active = self.active_app_index.unwrap_or(0);
        let js = format!(
            "if(typeof window.updateTabs==='function')window.updateTabs({},{});",
            serde_json::Value::Array(tabs),
            active
        );
        let _ = tb.evaluate_script(&js);
    }

    pub fn tab_bar_rect(&self, phys_width: u32) -> Rect {
        let tb_h = self.tab_bar_height_px();
        Rect {
            position: PhysicalPosition::new(0, 0).into(),
            size: PhysicalSize::new(phys_width, tb_h).into(),
        }
    }

    pub fn app_rect(&self, phys_width: u32, phys_height: u32) -> Rect {
        let tb_h = self.tab_bar_height_px();
        let app_height = phys_height.saturating_sub(tb_h);
        Rect {
            position: PhysicalPosition::new(0i32, tb_h as i32).into(),
            size: PhysicalSize::new(phys_width, app_height).into(),
        }
    }

    pub fn wallet_rect() -> Rect {
        // Use 1x1 instead of 0x0 so macOS WKWebView loads the page content.
        Rect {
            position: PhysicalPosition::new(0, 0).into(),
            size: PhysicalSize::new(1u32, 1u32).into(),
        }
    }

    /// Position the wallet webview as a floating panel in the bottom-right
    /// of the app area and make it visible.
    pub fn show_wallet_overlay(&self, phys_width: u32, phys_height: u32) {
        let wv = match &self.wallet {
            Some(wv) => wv,
            None => return,
        };
        let panel_w = (360.0 * self.scale_factor) as u32;
        let panel_h = (420.0 * self.scale_factor) as u32;
        let margin = (12.0 * self.scale_factor) as u32;
        let tb_h = self.tab_bar_height_px();
        let x = phys_width.saturating_sub(panel_w + margin) as i32;
        let y = phys_height.saturating_sub(panel_h + margin) as i32;
        // Clamp y so it doesn't overlap the tab bar
        let y = y.max(tb_h as i32);
        let rect = Rect {
            position: PhysicalPosition::new(x, y).into(),
            size: PhysicalSize::new(panel_w, panel_h).into(),
        };
        let _ = wv.set_bounds(rect);
        let _ = wv.set_visible(true);
        #[cfg(target_os = "macos")]
        bring_webview_to_front(wv);
    }

    /// Hide the wallet overlay and shrink it to zero-size.
    pub fn hide_wallet_overlay(&self) {
        let wv = match &self.wallet {
            Some(wv) => wv,
            None => return,
        };
        let _ = wv.set_visible(false);
        let _ = wv.set_bounds(Self::wallet_rect());
    }

    /// Send pairing URI + QR SVG to the wallet webview's JS.
    pub fn update_wallet_pairing(&self, uri: &str, qr_svg: &str) {
        let wv = match &self.wallet {
            Some(wv) => wv,
            None => return,
        };
        let uri_json = serde_json::to_string(uri).unwrap_or_else(|_| "\"\"".to_string());
        let qr_json = serde_json::to_string(qr_svg).unwrap_or_else(|_| "\"\"".to_string());
        let js = format!(
            "if(typeof window.showPairing==='function')window.showPairing({},{});",
            uri_json, qr_json
        );
        let _ = wv.evaluate_script(&js);
    }
}
