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
    emit_rerun_for_dir(&internal_ui.join("static"));

    let status = Command::new("bun")
        .arg("run")
        .arg("build")
        .current_dir(internal_ui)
        .status();

    match status {
        Ok(status) if status.success() => {}
        Ok(status) => panic!("internal-ui build failed with status: {status}"),
        Err(error) => panic!("failed to execute bun for internal-ui build: {error}"),
    }
}
