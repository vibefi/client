use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, BufRead, Write};
use tao::event_loop::EventLoopProxy;

use crate::state::UserEvent;
use crate::webview_manager::WebViewManager;

// ---------------------------------------------------------------------------
// NDJSON protocol types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct AutomationInput {
    id: String,
    #[serde(rename = "type")]
    cmd_type: String,
    target: Option<String>,
    js: Option<String>,
}

#[derive(Serialize)]
struct ResultMsg<'a> {
    id: &'a str,
    #[serde(rename = "type")]
    msg_type: &'static str,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WebviewInfo {
    id: String,
    kind: String,
    label: String,
}

// ---------------------------------------------------------------------------
// Stdout helpers (all output locked + flushed)
// ---------------------------------------------------------------------------

fn emit_line(value: &impl Serialize) {
    if let Ok(line) = serde_json::to_string(value) {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        let _ = writeln!(handle, "{}", line);
        let _ = handle.flush();
    }
}

pub fn emit_ready() {
    emit_line(&serde_json::json!({"type": "ready"}));
}

pub fn emit_webview_created(id: &str, kind: &str, label: &str) {
    emit_line(&serde_json::json!({
        "type": "webview_created",
        "webviewId": id,
        "kind": kind,
        "label": label,
    }));
}

pub fn emit_result(id: &str, ok: bool, value: Option<Value>, error: Option<String>) {
    emit_line(&ResultMsg {
        id,
        msg_type: "result",
        ok,
        value,
        error,
    });
}

fn emit_error(message: &str) {
    emit_line(&serde_json::json!({"type": "error", "message": message}));
}

// ---------------------------------------------------------------------------
// Stdin reader thread
// ---------------------------------------------------------------------------

pub fn spawn_stdin_reader(proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        let stdin = io::stdin();
        let reader = io::BufReader::new(stdin.lock());
        for line in reader.lines() {
            match line {
                Ok(line) if line.trim().is_empty() => continue,
                Ok(line) => match serde_json::from_str::<AutomationInput>(&line) {
                    Ok(cmd) => {
                        let _ = proxy.send_event(UserEvent::AutomationCommand {
                            id: cmd.id,
                            cmd_type: cmd.cmd_type,
                            target: cmd.target,
                            js: cmd.js,
                        });
                    }
                    Err(e) => emit_error(&format!("parse error: {e}")),
                },
                Err(_) => break, // stdin closed
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Command dispatch (runs on main/event-loop thread)
// ---------------------------------------------------------------------------

pub fn handle_command(
    id: String,
    cmd_type: String,
    target: Option<String>,
    js: Option<String>,
    manager: &WebViewManager,
) {
    match cmd_type.as_str() {
        "eval" => handle_eval(id, target, js, manager),
        "list_webviews" => handle_list_webviews(&id, manager),
        other => emit_result(&id, false, None, Some(format!("unknown command: {other}"))),
    }
}

fn handle_eval(
    id: String,
    target: Option<String>,
    js: Option<String>,
    manager: &WebViewManager,
) {
    let Some(target) = target else {
        emit_result(&id, false, None, Some("missing 'target' field".into()));
        return;
    };
    let Some(js) = js else {
        emit_result(&id, false, None, Some("missing 'js' field".into()));
        return;
    };
    let Some(webview) = manager.webview_for_id(&target) else {
        emit_result(
            &id,
            false,
            None,
            Some(format!("webview not found: {target}")),
        );
        return;
    };

    // Safely embed the automation id as a JSON string literal.
    let id_json = serde_json::to_string(&id).unwrap_or_else(|_| "\"?\"".into());

    let wrapped = format!(
        r#"(async()=>{{const __aid={id_json};try{{const __r=await(async()=>{{{js}}})();window.ipc.postMessage(JSON.stringify({{id:0,providerId:"vibefi-automation",method:"automation_result",params:{{automationId:__aid,ok:true,value:__r===undefined?null:__r}}}}))}}catch(e){{window.ipc.postMessage(JSON.stringify({{id:0,providerId:"vibefi-automation",method:"automation_result",params:{{automationId:__aid,ok:false,error:String(e)}}}}))}}}})()"#,
    );

    if let Err(e) = webview.evaluate_script(&wrapped) {
        emit_result(
            &id,
            false,
            None,
            Some(format!("evaluate_script failed: {e}")),
        );
    }
    // Result will arrive asynchronously via IPC → router → handle_automation_ipc_result.
}

fn handle_list_webviews(id: &str, manager: &WebViewManager) {
    let mut list = Vec::new();
    if manager.tab_bar.is_some() {
        list.push(WebviewInfo {
            id: "tab-bar".into(),
            kind: "TabBar".into(),
            label: "Tab Bar".into(),
        });
    }
    for entry in &manager.apps {
        list.push(WebviewInfo {
            id: entry.id.clone(),
            kind: format!("{:?}", entry.kind),
            label: entry.label.clone(),
        });
    }
    emit_result(id, true, Some(serde_json::to_value(list).unwrap()), None);
}

// ---------------------------------------------------------------------------
// IPC result handler (called from ipc/router.rs when vibefi-automation IPC
// arrives from a webview).
// ---------------------------------------------------------------------------

pub fn handle_automation_ipc_result(params: &Value) {
    let aid = params
        .get("automationId")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let ok = params.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    if ok {
        emit_result(aid, true, params.get("value").cloned(), None);
    } else {
        let error = params
            .get("error")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        emit_result(aid, false, None, error);
    }
}
