use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::state::{AppState, RunningCodeDevServer, UserEvent};

const START_PORT: u16 = 5199;
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

pub fn start_dev_server(
    state: &AppState,
    webview_id: &str,
    project_root: PathBuf,
) -> Result<Value> {
    if let Some(existing) = running_snapshot(state)? {
        if is_process_running(&existing.child)? {
            if existing.project_root == project_root {
                return Ok(status_json(Some(&existing), true));
            }
            bail!(
                "a code dev server is already running for {}; stop it before starting another",
                existing.project_root.display()
            );
        }
        clear_dev_server_if_matches(state, existing.id);
    }

    ensure_dependencies(&project_root, state, webview_id)?;
    let port = find_available_port(START_PORT)?;
    let ready = Arc::new(AtomicBool::new(false));

    let mut command = Command::new("bun");
    command
        .arg("x")
        .arg("vite")
        .arg("dev")
        .arg("--port")
        .arg(port.to_string())
        .arg("--host")
        .arg("localhost")
        .env("NO_COLOR", "1")
        .current_dir(&project_root)
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
        format!(
            "failed to spawn vite dev server in {}",
            project_root.display()
        )
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("failed to capture vite stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("failed to capture vite stderr"))?;

    let child = Arc::new(Mutex::new(child));
    let snapshot = install_running_server(
        state,
        webview_id,
        project_root.clone(),
        port,
        child,
        uses_process_group,
    )?;

    spawn_output_reader(
        state.clone(),
        state.proxy.clone(),
        snapshot.webview_id.clone(),
        "stdout",
        stdout,
        snapshot.port,
        snapshot.project_root.clone(),
        Arc::clone(&ready),
    );
    spawn_output_reader(
        state.clone(),
        state.proxy.clone(),
        snapshot.webview_id.clone(),
        "stderr",
        stderr,
        snapshot.port,
        snapshot.project_root.clone(),
        Arc::clone(&ready),
    );
    spawn_exit_watcher(state.clone(), snapshot, ready);

    let snapshot = running_snapshot(state)?;
    Ok(status_json(snapshot.as_ref(), true))
}

pub fn stop_dev_server(state: &AppState) -> Result<Value> {
    let Some(snapshot) = take_running_server(state)? else {
        return Ok(status_json(None, true));
    };

    let mut child = snapshot
        .child
        .lock()
        .map_err(|_| anyhow!("poisoned lock: code dev server child"))?;

    stop_child_process_tree(&mut child, snapshot.uses_process_group)?;

    Ok(status_json(None, true))
}

pub fn stop_dev_server_for_shutdown(state: &AppState) -> Result<()> {
    let Some(snapshot) = take_running_server(state)? else {
        return Ok(());
    };

    let mut child = snapshot
        .child
        .lock()
        .map_err(|_| anyhow!("poisoned lock: code dev server child"))?;
    stop_child_process_tree(&mut child, snapshot.uses_process_group)
}

pub fn dev_server_status(state: &AppState) -> Result<Value> {
    let Some(snapshot) = running_snapshot(state)? else {
        return Ok(status_json(None, true));
    };

    if is_process_running(&snapshot.child)? {
        return Ok(status_json(Some(&snapshot), true));
    }

    clear_dev_server_if_matches(state, snapshot.id);
    Ok(status_json(None, true))
}

fn ensure_dependencies(project_root: &Path, state: &AppState, webview_id: &str) -> Result<()> {
    let node_modules = project_root.join("node_modules");
    if node_modules.is_dir() {
        return Ok(());
    }

    emit_provider_event(
        &state.proxy,
        webview_id.to_string(),
        "codeConsoleOutput".to_string(),
        json!({
            "source": "system",
            "stream": "stdout",
            "line": "node_modules missing; running `bun install`...",
        }),
    );

    let output = Command::new("bun")
        .arg("install")
        .env("NO_COLOR", "1")
        .current_dir(project_root)
        .output()
        .with_context(|| format!("failed to run `bun install` in {}", project_root.display()))?;

    emit_buffered_output(&state.proxy, webview_id, "stdout", &output.stdout);
    emit_buffered_output(&state.proxy, webview_id, "stderr", &output.stderr);

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!(
        "`bun install` failed in {}: {}",
        project_root.display(),
        stderr.trim()
    );
}

fn emit_buffered_output(
    proxy: &tao::event_loop::EventLoopProxy<UserEvent>,
    webview_id: &str,
    stream: &str,
    bytes: &[u8],
) {
    for line in String::from_utf8_lossy(bytes).lines() {
        emit_provider_event(
            proxy,
            webview_id.to_string(),
            "codeConsoleOutput".to_string(),
            json!({
                "source": source_for_stream(stream),
                "stream": stream,
                "line": line,
            }),
        );
    }
}

fn find_available_port(start: u16) -> Result<u16> {
    let mut port = start;
    loop {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Ok(port);
        }
        if port == u16::MAX {
            bail!("no available port found starting at {}", start);
        }
        port = port.saturating_add(1);
    }
}

fn spawn_output_reader<R: std::io::Read + Send + 'static>(
    state: AppState,
    proxy: tao::event_loop::EventLoopProxy<UserEvent>,
    webview_id: String,
    stream: &'static str,
    reader: R,
    port: u16,
    project_root: PathBuf,
    ready: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        let reader = BufReader::new(reader);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    emit_provider_event(
                        &proxy,
                        webview_id.clone(),
                        "codeConsoleOutput".to_string(),
                        json!({
                            "source": source_for_stream(stream),
                            "stream": stream,
                            "line": line,
                        }),
                    );
                    maybe_emit_ready(&state, &proxy, &webview_id, port, &project_root, &ready, &line);
                }
                Err(err) => {
                    tracing::warn!(error = %err, stream, "failed to read dev server output");
                    break;
                }
            }
        }
    });
}

fn spawn_exit_watcher(state: AppState, snapshot: RunningSnapshot, _ready: Arc<AtomicBool>) {
    thread::spawn(move || {
        let status = loop {
            let check = match snapshot.child.lock() {
                Ok(mut child) => child.try_wait(),
                Err(_) => {
                    tracing::warn!("poisoned lock while waiting for dev server exit");
                    return;
                }
            };

            match check {
                Ok(Some(status)) => break status,
                Ok(None) => thread::sleep(Duration::from_millis(150)),
                Err(err) => {
                    tracing::warn!(error = %err, "failed while waiting for dev server exit");
                    return;
                }
            }
        };

        clear_dev_server_if_matches(&state, snapshot.id);

        emit_provider_event(
            &state.proxy,
            snapshot.webview_id,
            "codeDevServerExit".to_string(),
            json!({
                "port": snapshot.port,
                "projectPath": snapshot.project_root.to_string_lossy().to_string(),
                "code": status.code(),
                "success": status.success(),
            }),
        );
    });
}

fn maybe_emit_ready(
    state: &AppState,
    proxy: &tao::event_loop::EventLoopProxy<UserEvent>,
    webview_id: &str,
    port: u16,
    project_root: &Path,
    ready: &AtomicBool,
    output_line: &str,
) {
    if ready.load(Ordering::Relaxed) {
        return;
    }

    let Some(ready_port) = detect_ready_port(output_line, port) else {
        return;
    };

    if ready.swap(true, Ordering::SeqCst) {
        return;
    }

    update_running_server_port(state, webview_id, project_root, ready_port);

    if ready_port != port {
        emit_provider_event(
            proxy,
            webview_id.to_string(),
            "codeConsoleOutput".to_string(),
            json!({
                "source": "system",
                "stream": "stdout",
                "line": format!(
                    "vite selected localhost:{} instead of requested localhost:{}",
                    ready_port, port
                ),
            }),
        );
    }

    emit_provider_event(
        proxy,
        webview_id.to_string(),
        "codeDevServerReady".to_string(),
        json!({
            "port": ready_port,
            "url": format!("http://localhost:{}/", ready_port),
            "projectPath": project_root.to_string_lossy().to_string(),
        }),
    );
}

fn update_running_server_port(
    state: &AppState,
    webview_id: &str,
    project_root: &Path,
    ready_port: u16,
) {
    if let Ok(mut guard) = state.code.lock() {
        if let Some(server) = guard.dev_server.as_mut() {
            if server.webview_id == webview_id && server.project_root == project_root {
                server.port = ready_port;
            }
        }
    }
}

fn detect_ready_port(line: &str, configured_port: u16) -> Option<u16> {
    if let Some(port) = extract_port_after_prefix(line, "http://localhost:") {
        return Some(port);
    }
    if let Some(port) = extract_port_after_prefix(line, "http://127.0.0.1:") {
        return Some(port);
    }
    if line.contains(&format!("localhost:{configured_port}"))
        || line.contains(&format!("127.0.0.1:{configured_port}"))
    {
        return Some(configured_port);
    }
    None
}

fn extract_port_after_prefix(line: &str, prefix: &str) -> Option<u16> {
    let start = line.find(prefix)? + prefix.len();
    let digits: String = line[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u16>().ok()
}

fn running_snapshot(state: &AppState) -> Result<Option<RunningSnapshot>> {
    let guard = state
        .code
        .lock()
        .map_err(|_| anyhow!("poisoned lock: code"))?;
    Ok(guard.dev_server.as_ref().map(RunningSnapshot::from))
}

fn take_running_server(state: &AppState) -> Result<Option<RunningSnapshot>> {
    let mut guard = state
        .code
        .lock()
        .map_err(|_| anyhow!("poisoned lock: code"))?;
    Ok(guard
        .dev_server
        .take()
        .map(|server| RunningSnapshot::from(&server)))
}

fn install_running_server(
    state: &AppState,
    webview_id: &str,
    project_root: PathBuf,
    port: u16,
    child: Arc<Mutex<Child>>,
    uses_process_group: bool,
) -> Result<RunningSnapshot> {
    let mut guard = state
        .code
        .lock()
        .map_err(|_| anyhow!("poisoned lock: code"))?;

    let id = guard.next_dev_server_id;
    guard.next_dev_server_id = guard.next_dev_server_id.saturating_add(1);
    let server = RunningCodeDevServer {
        id,
        project_root,
        webview_id: webview_id.to_string(),
        port,
        child,
        uses_process_group,
    };
    let snapshot = RunningSnapshot::from(&server);
    guard.dev_server = Some(server);
    Ok(snapshot)
}

fn clear_dev_server_if_matches(state: &AppState, id: u64) {
    if let Ok(mut guard) = state.code.lock() {
        if guard.dev_server.as_ref().map(|server| server.id) == Some(id) {
            guard.dev_server = None;
        }
    }
}

fn status_json(server: Option<&RunningSnapshot>, ok: bool) -> Value {
    if let Some(server) = server {
        json!({
            "ok": ok,
            "running": true,
            "port": server.port,
            "url": format!("http://localhost:{}/", server.port),
            "projectPath": server.project_root.to_string_lossy().to_string(),
        })
    } else {
        json!({
            "ok": ok,
            "running": false,
            "port": Value::Null,
            "url": Value::Null,
            "projectPath": Value::Null,
        })
    }
}

fn is_process_running(child: &Arc<Mutex<Child>>) -> Result<bool> {
    let mut child = child
        .lock()
        .map_err(|_| anyhow!("poisoned lock: code dev server child"))?;
    Ok(child
        .try_wait()
        .context("failed to query dev server process")?
        .is_none())
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

fn source_for_stream(stream: &str) -> &'static str {
    match stream {
        "stderr" => "build",
        _ => "vite",
    }
}

impl From<&RunningCodeDevServer> for RunningSnapshot {
    fn from(server: &RunningCodeDevServer) -> Self {
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
        .context("failed to query dev server process")?
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

    bail!(
        "failed to stop dev server process tree for pid {}",
        child.id()
    );
}

fn wait_for_child_exit(child: &mut Child, timeout: Duration) -> Result<bool> {
    let start = std::time::Instant::now();
    loop {
        if child
            .try_wait()
            .context("failed to query dev server process")?
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
    child
        .kill()
        .context("failed to terminate dev server process")?;
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
    child
        .kill()
        .context("failed to force-kill dev server process")?;
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
