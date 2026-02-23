use tao::event_loop::EventLoopProxy;

use crate::state::UserEvent;
use crate::webview_manager::WebViewManager;

pub fn spawn_stdin_reader(_proxy: EventLoopProxy<UserEvent>) {}

pub fn emit_ready() {}

pub fn emit_webview_created(_id: &str, _kind: &str, _label: &str) {}

pub fn handle_command(
    _id: String,
    _cmd_type: String,
    _target: Option<String>,
    _js: Option<String>,
    _manager: &WebViewManager,
) {
}

pub fn handle_automation_ipc_result(_params: &serde_json::Value) {}
