use std::fs;
use std::path::Path;
use std::process::Command;

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

fn main() {
    let internal_ui = Path::new("internal-ui");
    emit_rerun_for_path(&internal_ui.join("package.json"));
    emit_rerun_for_path(&internal_ui.join("bun.lock"));
    emit_rerun_for_dir(&internal_ui.join("src"));
    emit_rerun_for_dir(&internal_ui.join("scripts"));
    emit_rerun_for_dir(&internal_ui.join("static"));

    println!("cargo:warning=[internal-ui] running: bun run build");
    let output = Command::new("bun")
        .arg("run")
        .arg("build")
        .current_dir(internal_ui)
        .output();

    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                println!("cargo:warning=[internal-ui][stdout] {line}");
            }

            let stderr = String::from_utf8_lossy(&output.stderr);
            for line in stderr.lines() {
                println!("cargo:warning=[internal-ui][stderr] {line}");
            }

            if !output.status.success() {
                panic!("internal-ui build failed with status: {}", output.status);
            }
        }
        Err(error) => panic!("failed to execute bun for internal-ui build: {error}"),
    }
}
