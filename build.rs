use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use brk_rolldown::{Bundler, BundlerOptions};
use brk_rolldown_common::bundler_options::{
    InputItem,
    OutputFormat,
    Platform,
    RawMinifyOptions,
};
use brk_rolldown_utils::indexmap::FxIndexMap;
use tokio::task::LocalSet;

fn emit_rerun_for_path(path: &Path) {
    if let Some(s) = path.to_str() {
        println!("cargo:rerun-if-changed={s}");
    }
}

fn emit_rerun_for_dir(dir: &Path) {
    emit_rerun_for_path(dir);
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                emit_rerun_for_dir(&path);
            } else {
                emit_rerun_for_path(&path);
            }
        }
    }
}

fn try_open_console() -> Option<std::fs::File> {
    #[cfg(unix)]
    let console_path = "/dev/tty";
    #[cfg(windows)]
    let console_path = "CONOUT$";
    OpenOptions::new().write(true).open(console_path).ok()
}

fn print_console_line(line: &str) {
    if let Some(mut console) = try_open_console() {
        let _ = writeln!(console, "{line}");
    }
}

fn run_bun_step(
    args: &[&str],
    cwd: &Path,
    start_message: &str,
    success_message: Option<&str>,
    step_label: &str,
) {
    print_console_line(start_message);

    let mut cmd = Command::new("bun");
    cmd.args(args).current_dir(cwd);

    run_with_console_handling(cmd, success_message, step_label);
}

fn run_with_console_handling(mut cmd: Command, success_message: Option<&str>, step_label: &str) {
    if let Some(console) = try_open_console() {
        let stdout_console = console
            .try_clone()
            .unwrap_or_else(|_| panic!("failed to clone console handle for {step_label} stdout"));
        cmd.stdout(Stdio::from(stdout_console));
        cmd.stderr(Stdio::from(console));

        let status = cmd
            .status()
            .unwrap_or_else(|_| panic!("failed to execute bun for {step_label}"));
        if !status.success() {
            panic!("{step_label} failed with status: {status}");
        }
        if let Some(msg) = success_message {
            print_console_line(msg);
        }
    } else {
        let output = cmd
            .output()
            .unwrap_or_else(|_| panic!("failed to execute bun for {step_label}"));
        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            panic!(
                "{step_label} failed with status: {}.\nstdout:\n{}\nstderr:\n{}",
                output.status, stdout, stderr
            );
        }
    }
}

fn build_internal_ui() -> Result<(), Box<dyn std::error::Error>> {
    let internal_ui = Path::new("internal-ui");
    let dist_dir = internal_ui.join("dist");

    // Create dist directory
    fs::create_dir_all(&dist_dir)?;

    // Install dependencies if node_modules doesn't exist
    let node_modules = internal_ui.join("node_modules");
    if !node_modules.exists() {
        print_console_line("[internal-ui] Installing dependencies with bun...");
        use std::process::Command;
        let output = Command::new("bun")
            .arg("install")
            .current_dir(internal_ui)
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("[internal-ui] bun install failed: {stderr}");
            return Err("bun install failed".into());
        }
    }

    // Define entry points matching the original build.ts
    let entries = vec![
        ("preload-app", "./internal-ui/src/preload-app.ts"),
        ("preload-wallet-selector", "./internal-ui/src/preload-wallet-selector.ts"),
        ("preload-tabbar", "./internal-ui/src/preload-tabbar.ts"),
        ("home", "./internal-ui/src/home.tsx"),
        ("launcher", "./internal-ui/src/launcher.tsx"),
        ("wallet-selector", "./internal-ui/src/wallet-selector.tsx"),
        ("tabbar", "./internal-ui/src/tabbar.tsx"),
        ("preload-settings", "./internal-ui/src/preload-settings.ts"),
        ("settings", "./internal-ui/src/settings.tsx"),
    ];

    print_console_line("[internal-ui] Building with Rolldown...");

    // Create tokio runtime once for all bundling operations
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let local = LocalSet::new();
    let current_dir = std::env::current_dir()?;
    let dist_dir_for_tasks = dist_dir.clone();

    let build_result: Result<(), io::Error> = local.block_on(&runtime, async move {
        let mut handles = Vec::new();

        for (name, entry) in entries {
            let task_name = name.to_string();
            let entry_path = entry.to_string();
            let dist_path = dist_dir_for_tasks.clone();
            let cwd = current_dir.clone();

            print_console_line(&format!("[internal-ui] Bundling {task_name}..."));

            handles.push(tokio::task::spawn_local(async move {
                let outfile = dist_path.join(format!("{task_name}.js"));

                // Configure Rolldown options to match Bun's build settings
                let mut define_map = FxIndexMap::default();
                define_map.insert("process.env.NODE_ENV".to_string(), "\"production\"".to_string());

                let options = BundlerOptions {
                    input: Some(vec![InputItem {
                        name: None,
                        import: entry_path,
                    }]),
                    cwd: Some(cwd),
                    platform: Some(Platform::Browser),
                    format: Some(OutputFormat::Iife),
                    file: Some(outfile.to_string_lossy().to_string()),
                    minify: Some(RawMinifyOptions::Bool(true)),
                    define: Some(define_map),
                    ..Default::default()
                };

                let mut bundler = Bundler::new(options)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to init bundler for {task_name}: {e:?}")))?;

                match bundler.write().await {
                    Ok(bundle_output) => {
                        for warning in &bundle_output.warnings {
                            print_console_line(&format!("[internal-ui] Warning ({task_name}): {warning:?}"));
                        }
                        print_console_line(&format!("[internal-ui] Finished bundling {task_name}"));
                        Ok(())
                    }
                    Err(e) => {
                        eprintln!("[internal-ui] Error building {task_name}: {e:?}");
                        Err(io::Error::new(io::ErrorKind::Other, format!("Failed to build {task_name}: {e:?}")))
                    }
                }
            }));
        }

        for handle in handles {
            handle
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Bundling task panicked: {e}")))??;
        }

        Ok(())
    });

    build_result?;

    print_console_line("[internal-ui] Rolldown build completed successfully");
    Ok(())
}

fn main() {
    let ipfs_helper = Path::new("ipfs-helper");
    emit_rerun_for_path(&ipfs_helper.join("package.json"));
    emit_rerun_for_path(&ipfs_helper.join("bun.lock"));
    emit_rerun_for_path(&ipfs_helper.join("index.mjs"));

    run_bun_step(
        &["install"],
        ipfs_helper,
        "[ipfs_helper] running: bun install",
        Some("[ipfs_helper] bun install completed successfully"),
        "ipfs_helper install",
    );

    let internal_ui = Path::new("internal-ui");
    emit_rerun_for_path(&internal_ui.join("package.json"));
    emit_rerun_for_path(&internal_ui.join("bun.lock"));
    emit_rerun_for_dir(&internal_ui.join("src"));
    emit_rerun_for_dir(&internal_ui.join("static"));
    println!("cargo:rerun-if-env-changed=SKIP_UI_BUILD");

    if std::env::var("SKIP_UI_BUILD").is_ok() {
        print_console_line("[internal-ui] SKIP_UI_BUILD set, skipping build");
        return;
    }

    if let Err(e) = build_internal_ui() {
        panic!("[internal-ui] Build failed: {e}");
    }
}
