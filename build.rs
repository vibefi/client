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

    print_console_line("[internal-ui] running: bun run build");

    let mut cmd = Command::new("bun");
    cmd.arg("run").arg("build").current_dir(internal_ui);

    if let Some(console) = try_open_console() {
        let stdout_console = console
            .try_clone()
            .expect("failed to clone console handle for bun stdout");
        cmd.stdout(Stdio::from(stdout_console));
        cmd.stderr(Stdio::from(console));

        let status = cmd
            .status()
            .expect("failed to execute bun for internal-ui build");
        if !status.success() {
            panic!("internal-ui build failed with status: {status}");
        }
        print_console_line("[internal-ui] bun build completed successfully");
        return;
    }

    // Fallback when no console is available (e.g., some CI/non-interactive environments).
    let output = cmd
        .output()
        .expect("failed to execute bun for internal-ui build");
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "internal-ui build failed with status: {}.\nstdout:\n{}\nstderr:\n{}",
            output.status, stdout, stderr
        );
    }
}
