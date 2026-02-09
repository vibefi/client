use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

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

fn run_with_console_handling(
    mut cmd: Command,
    success_message: Option<&str>,
    step_label: &str,
) {
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


fn main() {
    let internal_ui = Path::new("internal-ui");
    emit_rerun_for_path(&internal_ui.join("package.json"));
    emit_rerun_for_path(&internal_ui.join("bun.lock"));
    emit_rerun_for_dir(&internal_ui.join("src"));
    emit_rerun_for_dir(&internal_ui.join("scripts"));
    emit_rerun_for_dir(&internal_ui.join("static"));
    println!("cargo:rerun-if-env-changed=SKIP_UI_BUILD");

    if std::env::var("SKIP_UI_BUILD").is_ok() {
        print_console_line("[internal-ui] SKIP_UI_BUILD set, skipping bun build");
        return;
    }

    run_bun_step(
        &["install"],
        internal_ui,
        "[internal-ui] running: bun install",
        Some("[internal-ui] bun install completed successfully"),
        "internal-ui install",
    );

    run_bun_step(
        &["run", "build"],
        internal_ui,
        "[internal-ui] running: bun run build",
        Some("[internal-ui] bun build completed successfully"),
        "internal-ui build",
    );
}
