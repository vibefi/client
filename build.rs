use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use brk_rolldown::{Bundler, BundlerOptions};
use brk_rolldown_common::bundler_options::{
    InputItem,
    OutputFormat,
    Platform,
    RawMinifyOptions,
};
use brk_rolldown_utils::indexmap::FxIndexMap;

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

fn build_internal_ui() -> Result<(), Box<dyn std::error::Error>> {
    let internal_ui = Path::new("internal-ui");
    let dist_dir = internal_ui.join("dist");

    // Create dist directory
    fs::create_dir_all(&dist_dir)?;

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

    for (name, entry) in entries {
        print_console_line(&format!("[internal-ui] Bundling {name}..."));
        
        let outfile = dist_dir.join(format!("{name}.js"));
        
        // Configure Rolldown options to match Bun's build settings
        let mut define_map = FxIndexMap::default();
        define_map.insert("process.env.NODE_ENV".to_string(), "\"production\"".to_string());
        
        let options = BundlerOptions {
            input: Some(vec![InputItem {
                name: None,
                import: entry.to_string(),
            }]),
            cwd: Some(std::env::current_dir()?),
            platform: Some(Platform::Browser),
            format: Some(OutputFormat::Iife),
            file: Some(outfile.to_string_lossy().to_string()),
            minify: Some(RawMinifyOptions::Bool(true)),
            define: Some(define_map),
            ..Default::default()
        };

        // Build with Rolldown
        let mut bundler = Bundler::new(options)?;
        
        // Use tokio to run async
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        
        let output = runtime.block_on(async {
            bundler.write().await
        });

        match output {
            Ok(bundle_output) => {
                for warning in &bundle_output.warnings {
                    print_console_line(&format!("[internal-ui] Warning: {warning:?}"));
                }
            }
            Err(e) => {
                eprintln!("[internal-ui] Error building {name}: {e:?}");
                return Err(format!("Failed to build {name}").into());
            }
        }
    }

    print_console_line("[internal-ui] Rolldown build completed successfully");
    Ok(())
}

fn main() {
    let internal_ui = Path::new("internal-ui");
    emit_rerun_for_path(&internal_ui.join("package.json"));
    emit_rerun_for_dir(&internal_ui.join("src"));
    emit_rerun_for_dir(&internal_ui.join("scripts"));
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
