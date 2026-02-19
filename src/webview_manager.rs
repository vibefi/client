use std::path::PathBuf;
use wry::{Rect, WebView, dpi::PhysicalPosition, dpi::PhysicalSize};

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

// Suppress unused warning on non-macOS.
#[cfg(not(target_os = "macos"))]
fn bring_webview_to_front(_webview: &WebView) {}

/// Logical tab bar height in points. Must be scaled by the window's scale factor
/// to get the physical pixel height used in `Rect` bounds.
pub const TAB_BAR_HEIGHT_LOGICAL: f64 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppWebViewKind {
    Standard,
    Launcher,
    Studio,
    Code,
    WalletSelector,
    Settings,
}

impl AppWebViewKind {
    pub fn is_closeable(self) -> bool {
        !matches!(self, Self::Launcher | Self::Studio | Self::Code)
    }
}

pub struct AppWebViewEntry {
    pub webview: WebView,
    pub id: String,
    pub label: String,
    pub kind: AppWebViewKind,
    pub source_dir: Option<PathBuf>,
    pub selectable: bool,
    pub loading: bool,
}

pub struct WebViewManager {
    pub tab_bar: Option<WebView>,
    pub apps: Vec<AppWebViewEntry>,
    pub active_app_index: Option<usize>,
    next_id: u64,
    scale_factor: f64,
}

impl WebViewManager {
    pub fn new(scale_factor: f64) -> Self {
        Self {
            tab_bar: None,
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
        self.apps.iter().find(|e| e.id == id).map(|e| &e.webview)
    }

    pub fn active_app_webview(&self) -> Option<&WebView> {
        self.active_app_index
            .and_then(|i| self.apps.get(i))
            .map(|e| &e.webview)
    }

    pub fn switch_to(&mut self, index: usize) {
        if index >= self.apps.len() {
            tracing::debug!(
                index,
                app_count = self.apps.len(),
                "switch_to ignored out-of-range index"
            );
            return;
        }
        if !self.apps[index].selectable {
            tracing::debug!(index, "switch_to ignored for non-selectable tab");
            return;
        }
        if let Some(old) = self.active_app_index {
            if old < self.apps.len() {
                if let Err(err) = self.apps[old].webview.set_visible(false) {
                    tracing::warn!(index = old, error = %err, "failed to hide previous webview");
                }
            }
        }
        if let Err(err) = self.apps[index].webview.set_visible(true) {
            tracing::warn!(index, error = %err, "failed to show target webview");
        }
        #[cfg(target_os = "macos")]
        bring_webview_to_front(&self.apps[index].webview);
        self.active_app_index = Some(index);
        tracing::debug!(index, "switched active webview");
        self.update_tab_bar();
    }

    pub fn close_app(&mut self, index: usize) {
        if index >= self.apps.len() {
            tracing::debug!(
                index,
                app_count = self.apps.len(),
                "close_app ignored out-of-range index"
            );
            return;
        }
        if !self.apps[index].kind.is_closeable() {
            tracing::debug!(
                index,
                kind = ?self.apps[index].kind,
                "close_app ignored for non-closeable tab"
            );
            return;
        }
        // Don't close the last remaining tab.
        if self.apps.len() <= 1 {
            tracing::debug!("close_app ignored because only one tab exists");
            return;
        }
        self.apps.remove(index);
        tracing::debug!(index, remaining_tabs = self.apps.len(), "closed app tab");
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
            if let Err(err) = self.apps[i].webview.set_visible(true) {
                tracing::warn!(index = i, error = %err, "failed to show active webview after close");
            }
        }
        self.update_tab_bar();
    }

    pub fn index_of_kind(&self, kind: AppWebViewKind) -> Option<usize> {
        self.apps.iter().position(|e| e.kind == kind)
    }

    pub fn app_kind_for_id(&self, id: &str) -> Option<AppWebViewKind> {
        self.apps.iter().find(|e| e.id == id).map(|e| e.kind)
    }

    pub fn close_by_kind(&mut self, kind: AppWebViewKind) {
        if let Some(idx) = self.index_of_kind(kind) {
            self.close_app(idx);
        }
    }

    pub fn relayout(&self, phys_width: u32, phys_height: u32) {
        let tb_h = self.tab_bar_height_px();
        let tab_rect = Rect {
            position: PhysicalPosition::new(0, 0).into(),
            size: PhysicalSize::new(phys_width, tb_h).into(),
        };
        if let Some(tb) = &self.tab_bar {
            if let Err(err) = tb.set_bounds(tab_rect) {
                tracing::warn!(error = %err, "failed to set tab bar bounds");
            }
        }

        let app_height = phys_height.saturating_sub(tb_h);
        let app_rect = Rect {
            position: PhysicalPosition::new(0i32, tb_h as i32).into(),
            size: PhysicalSize::new(phys_width, app_height).into(),
        };
        for entry in &self.apps {
            if let Err(err) = entry.webview.set_bounds(app_rect) {
                tracing::warn!(id = %entry.id, error = %err, "failed to set app webview bounds");
            }
        }
    }

    pub fn update_tab_bar(&self) {
        let tb = match &self.tab_bar {
            Some(tb) => tb,
            None => return,
        };
        let has_code_tab = self.apps.iter().any(|entry| entry.kind == AppWebViewKind::Code);
        let tabs: Vec<serde_json::Value> = self
            .apps
            .iter()
            .map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "label": e.label,
                    "closable": e.kind.is_closeable(),
                    "clickable": e.selectable,
                    "loading": e.loading,
                    "forkable": has_code_tab && e.kind == AppWebViewKind::Standard && e.source_dir.is_some(),
                })
            })
            .collect();
        let active = self.active_app_index.unwrap_or(0);
        if let Err(err) = crate::ui_bridge::update_tabs(tb, tabs, active) {
            tracing::warn!(error = %err, "failed to update tab bar");
        }
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
}
