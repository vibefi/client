use alloy_signer_local::PrivateKeySigner;
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::code::settings as code_settings;
use crate::rpc_manager::{DEFAULT_MAX_CONCURRENT_RPC, RpcEndpoint, RpcEndpointManager};
use crate::state::{AppState, RunningCodeAnvil, UserEvent, WalletBackend};

const DEFAULT_ANVIL_HOST: &str = "127.0.0.1";
const DEFAULT_ANVIL_MNEMONIC: &str = "test test test test test test test test test test test junk";
const DEFAULT_ANVIL_FORK_URL: &str = "https://ethereum-rpc.publicnode.com";
const ANVIL_ACCOUNT_1_PRIVATE_KEY: &str =
    "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";
const SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_secs(2);

#[derive(Clone)]
struct RunningSnapshot {
    id: u64,
    port: u16,
    project_root: PathBuf,
    webview_id: String,
    child: Arc<Mutex<Child>>,
    uses_process_group: bool,
}

#[derive(Debug, Clone)]
struct EffectiveAnvilConfig {
    auto_start_on_open: bool,
    fork_url: String,
    port: u16,
    chain_id: u64,
}

pub fn auto_start_anvil_for_project(state: &AppState, webview_id: &str, project_root: PathBuf) -> Result<Value> {
    let cfg = effective_config(state)?;
    if !cfg.auto_start_on_open {
        return anvil_status(state);
    }
    start_anvil_with_config(state, webview_id, project_root, cfg, true)
}

pub fn start_anvil(state: &AppState, webview_id: &str, project_root: PathBuf) -> Result<Value> {
    let cfg = effective_config(state)?;
    start_anvil_with_config(state, webview_id, project_root, cfg, false)
}

pub fn stop_anvil(state: &AppState) -> Result<Value> {
    let Some(snapshot) = take_running_anvil(state)? else {
        restore_runtime_after_anvil(state);
        return Ok(status_json(state, None, true));
    };

    let mut child = snapshot
        .child
        .lock()
        .map_err(|_| anyhow!("poisoned lock: code anvil child"))?;
    stop_child_process_tree(&mut child, snapshot.uses_process_group)?;
    drop(child);

    restore_runtime_after_anvil(state);
    Ok(status_json(state, None, true))
}

pub fn stop_anvil_for_shutdown(state: &AppState) -> Result<()> {
    let Some(snapshot) = take_running_anvil(state)? else {
        restore_runtime_after_anvil(state);
        return Ok(());
    };

    let mut child = snapshot
        .child
        .lock()
        .map_err(|_| anyhow!("poisoned lock: code anvil child"))?;
    let result = stop_child_process_tree(&mut child, snapshot.uses_process_group);
    drop(child);
    restore_runtime_after_anvil(state);
    result
}

pub fn anvil_status(state: &AppState) -> Result<Value> {
    let Some(snapshot) = running_snapshot(state)? else {
        return Ok(status_json(state, None, true));
    };

    if is_process_running(&snapshot.child)? {
        return Ok(status_json(state, Some(&snapshot), true));
    }

    let _ = clear_anvil_if_matches(state, snapshot.id);
    restore_runtime_after_anvil(state);
    Ok(status_json(state, None, true))
}

fn start_anvil_with_config(
    state: &AppState,
    webview_id: &str,
    project_root: PathBuf,
    cfg: EffectiveAnvilConfig,
    auto: bool,
) -> Result<Value> {
    if let Some(existing) = running_snapshot(state)? {
        if is_process_running(&existing.child)? {
            if existing.project_root == project_root && existing.port == cfg.port {
                apply_runtime_for_anvil(state, cfg.port, cfg.chain_id)?;
                return Ok(status_json(state, Some(&existing), true));
            }
            stop_anvil(state)?;
        } else {
            clear_anvil_if_matches(state, existing.id);
            restore_runtime_after_anvil(state);
        }
    }

    let mut command = Command::new("anvil");
    command
        .arg("--host")
        .arg(DEFAULT_ANVIL_HOST)
        .arg("--port")
        .arg(cfg.port.to_string())
        .arg("--fork-url")
        .arg(&cfg.fork_url)
        .arg("--mnemonic")
        .arg(DEFAULT_ANVIL_MNEMONIC)
        .env("NO_COLOR", "1")
        .env("FOUNDRY_DISABLE_NIGHTLY_WARNING", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    let uses_process_group = {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        true
    };
    #[cfg(not(unix))]
    let uses_process_group = false;

    let mut child = command.spawn().with_context(|| {
        if auto {
            format!("failed to spawn anvil (auto-start) for {}", project_root.display())
        } else {
            format!("failed to spawn anvil for {}", project_root.display())
        }
    })?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("failed to capture anvil stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("failed to capture anvil stderr"))?;

    let child = Arc::new(Mutex::new(child));
    let snapshot = install_running_anvil(
        state,
        webview_id,
        project_root.clone(),
        cfg.port,
        Arc::clone(&child),
        uses_process_group,
    )?;

    let ready = Arc::new(AtomicBool::new(false));
    spawn_output_reader(
        state.clone(),
        snapshot.webview_id.clone(),
        "stdout",
        stdout,
        snapshot.port,
        snapshot.project_root.clone(),
        cfg.chain_id,
        Arc::clone(&ready),
    );
    spawn_output_reader(
        state.clone(),
        snapshot.webview_id.clone(),
        "stderr",
        stderr,
        snapshot.port,
        snapshot.project_root.clone(),
        cfg.chain_id,
        Arc::clone(&ready),
    );
    spawn_exit_watcher(state.clone(), snapshot, ready);

    let snapshot = running_snapshot(state)?;
    Ok(status_json(state, snapshot.as_ref(), true))
}

fn effective_config(state: &AppState) -> Result<EffectiveAnvilConfig> {
    let workspace_root = {
        let guard = state.code.lock().map_err(|_| anyhow!("poisoned lock: code"))?;
        guard.workspace_root.clone()
    };
    let code_settings = code_settings::load_settings(&workspace_root);
    let cfg = code_settings.anvil.normalized();
    let _ignored_user_fork_url = cfg.fork_url.clone();
    let _ignored_resolved_rpc_url = state
        .resolved
        .as_ref()
        .map(|resolved| resolved.rpc_url.trim().to_string());
    let fork_url = DEFAULT_ANVIL_FORK_URL.to_string();

    Ok(EffectiveAnvilConfig {
        auto_start_on_open: cfg.auto_start_on_open,
        fork_url,
        port: cfg.port,
        chain_id: cfg.chain_id,
    })
}

fn apply_runtime_for_anvil(state: &AppState, port: u16, chain_id: u64) -> Result<()> {
    let signer: PrivateKeySigner = ANVIL_ACCOUNT_1_PRIVATE_KEY
        .parse()
        .context("failed to parse anvil account #1 private key")?;
    let signer = Arc::new(signer);

    {
        let mut s = state.signer.lock().map_err(|_| anyhow!("poisoned lock: signer"))?;
        *s = Some(signer.clone());
    }
    {
        let mut wb = state
            .wallet_backend
            .lock()
            .map_err(|_| anyhow!("poisoned lock: wallet_backend"))?;
        *wb = Some(WalletBackend::Local);
    }
    {
        let mut wc = state
            .walletconnect
            .lock()
            .map_err(|_| anyhow!("poisoned lock: walletconnect"))?;
        *wc = None;
    }
    {
        let mut hs = state
            .hardware_signer
            .lock()
            .map_err(|_| anyhow!("poisoned lock: hardware_signer"))?;
        *hs = None;
    }
    {
        let mut ws = state.wallet.lock().map_err(|_| anyhow!("poisoned lock: wallet"))?;
        ws.authorized = false;
        ws.account = None;
        ws.walletconnect_uri = None;
        ws.chain.chain_id = chain_id;
    }

    let endpoint = RpcEndpoint {
        url: format!("http://{DEFAULT_ANVIL_HOST}:{port}"),
        label: Some("Code Anvil".to_string()),
    };

    let mut mgr = state
        .rpc_manager
        .lock()
        .map_err(|_| anyhow!("poisoned lock: rpc_manager"))?;
    if let Some(existing) = mgr.as_ref() {
        existing.set_endpoints(vec![endpoint]);
    } else {
        let http = if let Some(resolved) = state.resolved.as_ref() {
            resolved.http_client.clone()
        } else {
            reqwest::blocking::Client::new()
        };
        *mgr = Some(RpcEndpointManager::new(
            vec![endpoint],
            http,
            DEFAULT_MAX_CONCURRENT_RPC,
        ));
    }

    Ok(())
}

fn restore_runtime_after_anvil(state: &AppState) {
    if let Ok(mut signer) = state.signer.lock() {
        *signer = None;
    }
    if let Ok(mut wb) = state.wallet_backend.lock() {
        *wb = None;
    }
    if let Ok(mut wc) = state.walletconnect.lock() {
        *wc = None;
    }
    if let Ok(mut hs) = state.hardware_signer.lock() {
        *hs = None;
    }
    if let Ok(mut ws) = state.wallet.lock() {
        ws.authorized = false;
        ws.account = None;
        ws.walletconnect_uri = None;
        ws.chain.chain_id = state.resolved.as_ref().map(|r| r.chain_id).unwrap_or(1);
    }

    let mut mgr = match state.rpc_manager.lock() {
        Ok(mgr) => mgr,
        Err(_) => return,
    };

    let Some(resolved) = state.resolved.as_ref() else {
        return;
    };

    let endpoints = if let Some(config_path) = resolved.config_path.as_ref() {
        let user_settings = crate::settings::load_settings(config_path);
        if user_settings.rpc_endpoints.is_empty() {
            vec![RpcEndpoint {
                url: resolved.rpc_url.clone(),
                label: Some("Default".to_string()),
            }]
        } else {
            user_settings.rpc_endpoints
        }
    } else {
        vec![RpcEndpoint {
            url: resolved.rpc_url.clone(),
            label: Some("Default".to_string()),
        }]
    };

    if let Some(existing) = mgr.as_ref() {
        existing.set_endpoints(endpoints);
    } else {
        *mgr = Some(RpcEndpointManager::new(
            endpoints,
            resolved.http_client.clone(),
            DEFAULT_MAX_CONCURRENT_RPC,
        ));
    }
}

fn spawn_output_reader<R: std::io::Read + Send + 'static>(
    state: AppState,
    webview_id: String,
    stream: &'static str,
    reader: R,
    port: u16,
    project_root: PathBuf,
    chain_id: u64,
    ready: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        let reader = BufReader::new(reader);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    emit_provider_event(
                        &state.proxy,
                        webview_id.clone(),
                        "codeConsoleOutput".to_string(),
                        json!({
                            "source": "anvil",
                            "stream": stream,
                            "line": line,
                        }),
                    );
                    tracing::debug!(target: "vibefi::code::anvil", stream, line = %line, "anvil output");
                    maybe_emit_ready(
                        &state,
                        &webview_id,
                        port,
                        chain_id,
                        &project_root,
                        &ready,
                        &line,
                    );
                }
                Err(err) => {
                    tracing::warn!(error = %err, stream, "failed to read anvil output");
                    break;
                }
            }
        }
    });
}

fn maybe_emit_ready(
    state: &AppState,
    webview_id: &str,
    port: u16,
    chain_id: u64,
    project_root: &Path,
    ready: &AtomicBool,
    output_line: &str,
) {
    if ready.load(Ordering::Relaxed) {
        return;
    }
    if !output_line.contains("Listening on") {
        return;
    }
    if ready.swap(true, Ordering::SeqCst) {
        return;
    }

    if let Err(error) = apply_runtime_for_anvil(state, port, chain_id) {
        emit_provider_event(
            &state.proxy,
            webview_id.to_string(),
            "codeAnvilError".to_string(),
            json!({
                "message": format!("Failed to activate local Anvil wallet/provider: {}", error),
                "projectPath": project_root.to_string_lossy().to_string(),
            }),
        );
        tracing::warn!(error = %error, "failed to activate runtime for anvil");
        return;
    }

    let account = state.local_signer_address();
    emit_provider_event(
        &state.proxy,
        webview_id.to_string(),
        "codeAnvilReady".to_string(),
        json!({
            "port": port,
            "url": format!("http://{DEFAULT_ANVIL_HOST}:{port}"),
            "chainId": chain_id,
            "account": account,
            "accountIndex": 1,
            "projectPath": project_root.to_string_lossy().to_string(),
        }),
    );
}

fn spawn_exit_watcher(state: AppState, snapshot: RunningSnapshot, _ready: Arc<AtomicBool>) {
    thread::spawn(move || {
        let status = loop {
            let check = match snapshot.child.lock() {
                Ok(mut child) => child.try_wait(),
                Err(_) => {
                    tracing::warn!("poisoned lock while waiting for anvil exit");
                    return;
                }
            };

            match check {
                Ok(Some(status)) => break status,
                Ok(None) => thread::sleep(Duration::from_millis(150)),
                Err(err) => {
                    tracing::warn!(error = %err, "failed while waiting for anvil exit");
                    return;
                }
            }
        };

        let cleared = clear_anvil_if_matches(&state, snapshot.id);
        let was_ready = _ready.load(Ordering::Relaxed);
        if cleared {
            restore_runtime_after_anvil(&state);
        }

        if !was_ready {
            emit_provider_event(
                &state.proxy,
                snapshot.webview_id.clone(),
                "codeAnvilError".to_string(),
                json!({
                    "message": "Anvil exited before becoming ready. Check the Code console (anvil output) for the startup error (common causes: invalid fork URL or port already in use).",
                    "projectPath": snapshot.project_root.to_string_lossy().to_string(),
                    "port": snapshot.port,
                    "code": status.code(),
                    "success": status.success(),
                }),
            );
        }

        emit_provider_event(
            &state.proxy,
            snapshot.webview_id,
            "codeAnvilExit".to_string(),
            json!({
                "port": snapshot.port,
                "url": format!("http://{DEFAULT_ANVIL_HOST}:{port}", port = snapshot.port),
                "projectPath": snapshot.project_root.to_string_lossy().to_string(),
                "code": status.code(),
                "success": status.success(),
            }),
        );
    });
}

fn running_snapshot(state: &AppState) -> Result<Option<RunningSnapshot>> {
    let guard = state.code.lock().map_err(|_| anyhow!("poisoned lock: code"))?;
    Ok(guard.anvil.as_ref().map(RunningSnapshot::from))
}

fn take_running_anvil(state: &AppState) -> Result<Option<RunningSnapshot>> {
    let mut guard = state.code.lock().map_err(|_| anyhow!("poisoned lock: code"))?;
    Ok(guard.anvil.take().map(|anvil| RunningSnapshot::from(&anvil)))
}

fn install_running_anvil(
    state: &AppState,
    webview_id: &str,
    project_root: PathBuf,
    port: u16,
    child: Arc<Mutex<Child>>,
    uses_process_group: bool,
) -> Result<RunningSnapshot> {
    let mut guard = state.code.lock().map_err(|_| anyhow!("poisoned lock: code"))?;
    let id = guard.next_anvil_id;
    guard.next_anvil_id = guard.next_anvil_id.saturating_add(1);
    let anvil = RunningCodeAnvil {
        id,
        project_root,
        webview_id: webview_id.to_string(),
        port,
        child,
        uses_process_group,
    };
    let snapshot = RunningSnapshot::from(&anvil);
    guard.anvil = Some(anvil);
    Ok(snapshot)
}

fn clear_anvil_if_matches(state: &AppState, id: u64) -> bool {
    if let Ok(mut guard) = state.code.lock() {
        if guard.anvil.as_ref().map(|anvil| anvil.id) == Some(id) {
            guard.anvil = None;
            return true;
        }
    }
    false
}

fn status_json(state: &AppState, running: Option<&RunningSnapshot>, ok: bool) -> Value {
    let code_cfg = {
        let workspace_root = match state.code.lock() {
            Ok(guard) => guard.workspace_root.clone(),
            Err(_) => PathBuf::new(),
        };
        if workspace_root.as_os_str().is_empty() {
            code_settings::CodeAnvilConfig::default()
        } else {
            code_settings::load_settings(&workspace_root).anvil.normalized()
        }
    };

    let account = state.local_signer_address();

    if let Some(anvil) = running {
        json!({
            "ok": ok,
            "running": true,
            "port": anvil.port,
            "url": format!("http://{DEFAULT_ANVIL_HOST}:{port}", port = anvil.port),
            "projectPath": anvil.project_root.to_string_lossy().to_string(),
            "account": account,
            "accountIndex": 1,
            "chainId": state.wallet.lock().ok().map(|ws| ws.chain.chain_id).unwrap_or(code_cfg.chain_id),
            "config": code_cfg,
        })
    } else {
        json!({
            "ok": ok,
            "running": false,
            "port": Value::Null,
            "url": Value::Null,
            "projectPath": Value::Null,
            "account": account,
            "accountIndex": 1,
            "chainId": code_cfg.chain_id,
            "config": code_cfg,
        })
    }
}

fn is_process_running(child: &Arc<Mutex<Child>>) -> Result<bool> {
    let mut child = child
        .lock()
        .map_err(|_| anyhow!("poisoned lock: code anvil child"))?;
    Ok(child.try_wait().context("failed to query anvil process")?.is_none())
}

fn emit_provider_event(
    proxy: &tao::event_loop::EventLoopProxy<UserEvent>,
    webview_id: String,
    event: String,
    value: Value,
) {
    let _ = proxy.send_event(UserEvent::ProviderEvent {
        webview_id,
        event,
        value,
    });
}

impl From<&RunningCodeAnvil> for RunningSnapshot {
    fn from(server: &RunningCodeAnvil) -> Self {
        Self {
            id: server.id,
            port: server.port,
            project_root: server.project_root.clone(),
            webview_id: server.webview_id.clone(),
            child: Arc::clone(&server.child),
            uses_process_group: server.uses_process_group,
        }
    }
}

fn stop_child_process_tree(child: &mut Child, uses_process_group: bool) -> Result<()> {
    if child
        .try_wait()
        .context("failed to query anvil process")?
        .is_some()
    {
        return Ok(());
    }

    terminate_child_tree(child, uses_process_group)?;
    if wait_for_child_exit(child, SHUTDOWN_GRACE_PERIOD)? {
        return Ok(());
    }

    force_kill_child_tree(child, uses_process_group)?;
    if wait_for_child_exit(child, SHUTDOWN_GRACE_PERIOD)? {
        return Ok(());
    }

    bail!("failed to stop anvil process tree for pid {}", child.id())
}

fn wait_for_child_exit(child: &mut Child, timeout: Duration) -> Result<bool> {
    let start = std::time::Instant::now();
    loop {
        if child
            .try_wait()
            .context("failed to query anvil process")?
            .is_some()
        {
            return Ok(true);
        }
        if start.elapsed() >= timeout {
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn terminate_child_tree(child: &mut Child, uses_process_group: bool) -> Result<()> {
    #[cfg(unix)]
    {
        if uses_process_group && signal_unix_process_group(child.id(), "TERM")? {
            return Ok(());
        }
    }
    #[cfg(windows)]
    {
        let _ = taskkill_pid(child.id(), false);
    }
    child.kill().context("failed to terminate anvil process")?;
    Ok(())
}

fn force_kill_child_tree(child: &mut Child, uses_process_group: bool) -> Result<()> {
    #[cfg(unix)]
    {
        if uses_process_group && signal_unix_process_group(child.id(), "KILL")? {
            return Ok(());
        }
    }
    #[cfg(windows)]
    {
        let _ = taskkill_pid(child.id(), true);
    }
    child.kill().context("failed to force-kill anvil process")?;
    Ok(())
}

#[cfg(unix)]
fn signal_unix_process_group(pid: u32, signal: &str) -> Result<bool> {
    let target = format!("-{pid}");
    let status = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg("--")
        .arg(target)
        .status()
        .with_context(|| format!("failed to send SIG{signal} to process group {pid}"))?;
    Ok(status.success())
}

#[cfg(windows)]
fn taskkill_pid(pid: u32, force: bool) -> Result<bool> {
    let mut cmd = Command::new("taskkill");
    cmd.arg("/PID").arg(pid.to_string()).arg("/T");
    if force {
        cmd.arg("/F");
    }
    let status = cmd
        .status()
        .with_context(|| format!("failed to run taskkill for pid {pid}"))?;
    Ok(status.success())
}
