use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;
use std::path::{Path, PathBuf};

use crate::ipc_contract::IpcRequest;
use crate::state::{AppState, UserEvent};
use crate::webview_manager::{AppWebViewKind, WebViewManager};

use super::{anvil, dev_server, filesystem, project, settings, validator};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListFilesParams {
    #[serde(default)]
    project_path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadFileParams {
    project_path: String,
    file_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchParams {
    project_path: String,
    query: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WriteFileParams {
    project_path: String,
    file_path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteFileParams {
    project_path: String,
    file_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteDirParams {
    project_path: String,
    dir_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateDirParams {
    project_path: String,
    dir_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenameFileParams {
    project_path: String,
    old_path: String,
    new_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ValidateProjectParams {
    project_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateProjectParams {
    name: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenProjectParams {
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartDevServerParams {
    #[serde(default, alias = "path")]
    project_path: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartAnvilParams {
    #[serde(default, alias = "path")]
    project_path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ForkDappParams {
    webview_id: String,
    #[serde(default)]
    name: Option<String>,
}

pub fn handle_code_ipc(
    state: &AppState,
    manager: &WebViewManager,
    webview_id: &str,
    req: &IpcRequest,
) -> Result<Option<serde_json::Value>> {
    match req.method.as_str() {
        "code_listFiles" => {
            let params: ListFilesParams = parse_params(req)?;
            let project_path = resolve_project_path(state, params.project_path)?;
            let project_root = filesystem::resolve_project_root(&project_path)?;
            let files = filesystem::list_files(&project_root)?;
            set_active_project(state, project_root);
            Ok(Some(json!({ "files": files })))
        }
        "code_readFile" => {
            let params: ReadFileParams = parse_params(req)?;
            let project_root = filesystem::resolve_project_root(&params.project_path)?;
            let content = filesystem::read_file(&project_root, &params.file_path)?;
            set_active_project(state, project_root);
            Ok(Some(json!({ "content": content })))
        }
        "code_search" => {
            let params: SearchParams = parse_params(req)?;
            let project_root = filesystem::resolve_project_root(&params.project_path)?;
            let results = filesystem::search(&project_root, &params.query)?;
            set_active_project(state, project_root);
            Ok(Some(json!({ "results": results })))
        }
        "code_writeFile" => {
            let params: WriteFileParams = parse_params(req)?;
            let project_root = filesystem::resolve_project_root(&params.project_path)?;
            let write_kind = filesystem::write_file(&project_root, &params.file_path, &params.content)?;
            let event_kind = match write_kind {
                filesystem::WriteFileKind::Create => "create",
                filesystem::WriteFileKind::Modify => "modify",
            };
            emit_file_changed(state, webview_id, &params.file_path, event_kind);
            emit_project_validation_console(state, webview_id, &project_root);
            set_active_project(state, project_root);
            Ok(Some(json!({ "ok": true })))
        }
        "code_deleteFile" => {
            let params: DeleteFileParams = parse_params(req)?;
            let project_root = filesystem::resolve_project_root(&params.project_path)?;
            filesystem::delete_file(&project_root, &params.file_path)?;
            emit_file_changed(state, webview_id, &params.file_path, "delete");
            emit_project_validation_console(state, webview_id, &project_root);
            set_active_project(state, project_root);
            Ok(Some(json!({ "ok": true })))
        }
        "code_deleteDir" => {
            let params: DeleteDirParams = parse_params(req)?;
            let project_root = filesystem::resolve_project_root(&params.project_path)?;
            filesystem::delete_dir(&project_root, &params.dir_path)?;
            emit_file_changed(state, webview_id, &params.dir_path, "delete");
            emit_project_validation_console(state, webview_id, &project_root);
            set_active_project(state, project_root);
            Ok(Some(json!({ "ok": true })))
        }
        "code_createDir" => {
            let params: CreateDirParams = parse_params(req)?;
            let project_root = filesystem::resolve_project_root(&params.project_path)?;
            filesystem::create_dir(&project_root, &params.dir_path)?;
            emit_file_changed(state, webview_id, &params.dir_path, "create");
            emit_project_validation_console(state, webview_id, &project_root);
            set_active_project(state, project_root);
            Ok(Some(json!({ "ok": true })))
        }
        "code_renameFile" => {
            let params: RenameFileParams = parse_params(req)?;
            let project_root = filesystem::resolve_project_root(&params.project_path)?;
            filesystem::rename_file(&project_root, &params.old_path, &params.new_path)?;
            emit_file_changed(state, webview_id, &params.old_path, "delete");
            emit_file_changed(state, webview_id, &params.new_path, "create");
            set_active_project(state, project_root);
            Ok(Some(json!({ "ok": true })))
        }
        "code_validateProject" => {
            let params: ValidateProjectParams = parse_params(req)?;
            let project_root = filesystem::resolve_project_root(&params.project_path)?;
            let errors = validator::validate_project(&project_root)?;
            let valid = validator::is_valid(&errors);
            set_active_project(state, project_root);
            Ok(Some(json!({ "valid": valid, "errors": errors })))
        }
        "code_createProject" => {
            let params: CreateProjectParams = parse_params(req)?;
            let workspace_root = resolve_workspace_root(state)?;
            let project_root = project::create_project(&workspace_root, &params.name)?;
            let project_path = project_root.to_string_lossy().into_owned();
            set_active_project(state, project_root.clone());
            persist_last_project_path(state, &project_root);
            Ok(Some(json!({ "projectPath": project_path })))
        }
        "code_listProjects" => {
            let workspace_root = resolve_workspace_root(state)?;
            let projects = project::list_projects(&workspace_root)?;
            Ok(Some(json!({ "projects": projects })))
        }
        "code_openProject" => {
            let params: OpenProjectParams = parse_params_or_default(req)?;
            let workspace_root = resolve_workspace_root(state)?;

            let project_root = match params.path {
                Some(path) => project::resolve_open_project_path(&workspace_root, &path)?,
                None => {
                    let active_project = current_active_project(state)?;
                    if let Some(project_root) = active_project {
                        project::validate_project_root(&project_root)?;
                        project_root
                    } else {
                        let code_settings = settings::load_settings(&workspace_root);
                        let remembered_path = code_settings.last_project_path.ok_or_else(|| {
                            anyhow!(
                                "code_openProject requires a project path (no active or remembered project)"
                            )
                        })?;
                        project::resolve_open_project_path(&workspace_root, &remembered_path)?
                    }
                }
            };

            let files = filesystem::list_files(&project_root)?;
            let project_path = project_root.to_string_lossy().into_owned();
            if let Err(error) = project::ensure_preview_console_bridge(&project_root) {
                tracing::warn!(
                    error = %error,
                    project = %project_root.display(),
                    "failed to ensure preview console bridge for opened project"
                );
            }
            set_active_project(state, project_root.clone());
            persist_last_project_path(state, &project_root);
            if let Err(error) = anvil::auto_start_anvil_for_project(state, webview_id, project_root.clone())
            {
                tracing::warn!(
                    error = %error,
                    project = %project_root.display(),
                    "failed to auto-start anvil for opened project"
                );
                emit_code_provider_event(
                    state,
                    webview_id,
                    "codeAnvilError",
                    json!({
                        "message": error.to_string(),
                        "projectPath": project_root.to_string_lossy().to_string(),
                    }),
                );
            }
            Ok(Some(json!({ "projectPath": project_path, "files": files })))
        }
        "code_startDevServer" => {
            let params: StartDevServerParams = parse_params_or_default(req)?;
            let project_root = resolve_dev_server_project_root(state, params.project_path)?;
            if let Err(error) = project::ensure_preview_console_bridge(&project_root) {
                tracing::warn!(
                    error = %error,
                    project = %project_root.display(),
                    "failed to ensure preview console bridge before starting dev server"
                );
            }
            let response = dev_server::start_dev_server(state, webview_id, project_root.clone())?;
            set_active_project(state, project_root);
            Ok(Some(response))
        }
        "code_stopDevServer" => {
            let response = dev_server::stop_dev_server(state)?;
            Ok(Some(response))
        }
        "code_devServerStatus" => {
            let response = dev_server::dev_server_status(state)?;
            Ok(Some(response))
        }
        "code_startAnvil" => {
            let params: StartAnvilParams = parse_params_or_default(req)?;
            let project_root = resolve_dev_server_project_root(state, params.project_path)?;
            let response = anvil::start_anvil(state, webview_id, project_root.clone())?;
            set_active_project(state, project_root);
            Ok(Some(response))
        }
        "code_stopAnvil" => {
            let response = anvil::stop_anvil(state)?;
            Ok(Some(response))
        }
        "code_anvilStatus" => {
            let response = anvil::anvil_status(state)?;
            Ok(Some(response))
        }
        "code_getAnvilConfig" => {
            let workspace_root = resolve_workspace_root(state)?;
            let code_settings = settings::load_settings(&workspace_root);
            Ok(Some(serde_json::to_value(code_settings.anvil)?))
        }
        "code_setAnvilConfig" => {
            let workspace_root = resolve_workspace_root(state)?;
            let params: settings::CodeAnvilConfig = parse_params(req)?;
            let mut code_settings = settings::load_settings(&workspace_root);
            code_settings.anvil = params.normalized();
            settings::save_settings(&workspace_root, &code_settings)?;
            Ok(Some(serde_json::to_value(code_settings.anvil)?))
        }
        "code_forkDapp" => {
            let params: ForkDappParams = parse_params(req)?;
            let target_webview_id = params.webview_id.trim();
            if target_webview_id.is_empty() {
                bail!("webviewId must not be empty");
            }

            let source_entry = manager
                .apps
                .iter()
                .find(|entry| entry.id == target_webview_id)
                .ok_or_else(|| anyhow!("unknown webview id: {}", target_webview_id))?;
            if source_entry.kind != AppWebViewKind::Standard {
                bail!("only standard dapp tabs can be forked");
            }

            let source_dir = source_entry
                .source_dir
                .as_ref()
                .ok_or_else(|| anyhow!("Source not available for this dapp"))?;
            let workspace_root = resolve_workspace_root(state)?;
            let forked_project = project::fork_project_from_source(
                &workspace_root,
                source_dir,
                params.name.as_deref().or(Some(source_entry.label.as_str())),
            )?;
            if let Err(error) = project::ensure_preview_console_bridge(&forked_project) {
                tracing::warn!(
                    error = %error,
                    project = %forked_project.display(),
                    "failed to ensure preview console bridge for forked project"
                );
            }
            let forked_project_path = forked_project.to_string_lossy().into_owned();
            set_active_project(state, forked_project.clone());
            persist_last_project_path(state, &forked_project);

            let _ = state.proxy.send_event(UserEvent::ProviderEvent {
                webview_id: webview_id.to_string(),
                event: "codeForkComplete".to_string(),
                value: json!({
                    "projectPath": forked_project_path,
                    "sourceWebviewId": target_webview_id,
                }),
            });

            Ok(Some(json!({ "projectPath": forked_project_path })))
        }
        _ => bail!("unsupported code method: {}", req.method),
    }
}

fn parse_params<T: DeserializeOwned>(req: &IpcRequest) -> Result<T> {
    let payload = if req.params.is_array() {
        req.params
            .as_array()
            .and_then(|values| values.first().cloned())
            .ok_or_else(|| anyhow!("missing params"))?
    } else {
        req.params.clone()
    };

    serde_json::from_value(payload).with_context(|| format!("invalid params for {}", req.method))
}

fn parse_params_or_default<T: DeserializeOwned + Default>(req: &IpcRequest) -> Result<T> {
    if req.params.is_null() {
        return Ok(T::default());
    }
    if req
        .params
        .as_array()
        .map(|values| values.is_empty())
        .unwrap_or(false)
    {
        return Ok(T::default());
    }
    parse_params(req)
}

fn resolve_project_path(state: &AppState, provided: Option<String>) -> Result<String> {
    if let Some(project_path) = provided {
        if !project_path.trim().is_empty() {
            return Ok(project_path);
        }
    }

    let workspace = resolve_workspace_root(state)?;
    Ok(workspace.to_string_lossy().into_owned())
}

fn resolve_dev_server_project_root(state: &AppState, provided: Option<String>) -> Result<PathBuf> {
    if let Some(project_path) = provided {
        if !project_path.trim().is_empty() {
            return filesystem::resolve_project_root(&project_path);
        }
    }

    let active_project = current_active_project(state)?;
    let Some(project_root) = active_project else {
        bail!("code_startDevServer requires an active project or explicit projectPath");
    };

    project::validate_project_root(&project_root)?;
    Ok(project_root)
}

fn resolve_workspace_root(state: &AppState) -> Result<PathBuf> {
    let guard = state
        .code
        .lock()
        .map_err(|_| anyhow!("poisoned lock: code"))?;
    let workspace = guard.workspace_root.clone();
    drop(guard);

    std::fs::create_dir_all(&workspace).with_context(|| {
        format!(
            "failed to create code workspace root {}",
            workspace.display()
        )
    })?;
    Ok(workspace)
}

fn current_active_project(state: &AppState) -> Result<Option<PathBuf>> {
    let guard = state
        .code
        .lock()
        .map_err(|_| anyhow!("poisoned lock: code"))?;
    Ok(guard.active_project.clone())
}

fn set_active_project(state: &AppState, project_root: PathBuf) {
    if let Ok(mut guard) = state.code.lock() {
        guard.active_project = Some(project_root);
    }
}

fn persist_last_project_path(state: &AppState, project_root: &Path) {
    let workspace_root = match resolve_workspace_root(state) {
        Ok(path) => path,
        Err(error) => {
            tracing::warn!(error = %error, "failed to resolve code workspace root for last project persistence");
            return;
        }
    };

    let mut code_settings = settings::load_settings(&workspace_root);
    code_settings.last_project_path = Some(project_root.to_string_lossy().into_owned());
    if let Err(error) = settings::save_settings(&workspace_root, &code_settings) {
        tracing::warn!(
            error = %error,
            project = %project_root.display(),
            "failed to persist last opened code project"
        );
    }
}

fn emit_file_changed(state: &AppState, webview_id: &str, path: &str, kind: &str) {
    let _ = state.proxy.send_event(UserEvent::ProviderEvent {
        webview_id: webview_id.to_string(),
        event: "codeFileChanged".to_string(),
        value: json!({
            "path": path,
            "kind": kind,
        }),
    });
}

fn emit_project_validation_console(state: &AppState, webview_id: &str, project_root: &Path) {
    match validator::validate_project(project_root) {
        Ok(errors) => {
            for error in errors {
                emit_code_console_output(
                    state,
                    webview_id,
                    "lint",
                    &format_validation_console_line(&error),
                );
            }
        }
        Err(error) => {
            emit_code_console_output(
                state,
                webview_id,
                "lint",
                &format!("[error] validation failed: {}", error),
            );
        }
    }
}

fn emit_code_console_output(state: &AppState, webview_id: &str, source: &str, line: &str) {
    emit_code_provider_event(
        state,
        webview_id,
        "codeConsoleOutput",
        json!({
            "source": source,
            "line": line,
        }),
    );
}

fn emit_code_provider_event(
    state: &AppState,
    webview_id: &str,
    event: &str,
    value: serde_json::Value,
) {
    let _ = state.proxy.send_event(UserEvent::ProviderEvent {
        webview_id: webview_id.to_string(),
        event: event.to_string(),
        value,
    });
}

fn format_validation_console_line(error: &validator::ValidationError) -> String {
    let severity = match error.severity {
        validator::ValidationSeverity::Error => "error",
        validator::ValidationSeverity::Warning => "warning",
    };

    let location = match (&error.file, error.line) {
        (Some(file), Some(line)) => format!("{file}:{line}"),
        (Some(file), None) => file.clone(),
        _ => "<project>".to_string(),
    };

    format!(
        "[{}] {}: {} (rule: {})",
        severity, location, error.message, error.rule
    )
}
