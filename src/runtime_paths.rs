use anyhow::{Result, bail};
use std::env;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Returns the path to the app bundle's `Contents/` directory on macOS,
/// or `None` if the current executable is not inside an `.app` bundle.
fn macos_bundle_contents_dir() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    // Typical layout: Foo.app/Contents/MacOS/vibefi
    let macos_dir = exe.parent()?;
    if macos_dir.file_name()?.to_str()? != "MacOS" {
        return None;
    }
    let contents = macos_dir.parent()?;
    if contents.file_name()?.to_str()? != "Contents" {
        return None;
    }
    Some(contents.to_path_buf())
}

/// Resolve the Node/Bun runtime binary.
///
/// Resolution order:
/// 1. `VIBEFI_NODE_BIN` environment variable
/// 2. Bundled binary inside macOS app bundle (`Contents/MacOS/bun`)
/// 3. PATH probe (`bun`, then `node`) for development
pub fn resolve_node_binary() -> Result<String> {
    // 1. Explicit env override
    if let Ok(bin) = env::var("VIBEFI_NODE_BIN") {
        if !bin.trim().is_empty() {
            return Ok(bin);
        }
    }

    // 2. Bundled binary in app bundle
    if let Some(contents) = macos_bundle_contents_dir() {
        let bundled = contents.join("MacOS").join("bun");
        if bundled.exists() {
            return Ok(bundled.to_string_lossy().into_owned());
        }
    }

    // 3. PATH fallback (dev mode)
    for candidate in ["bun", "node"] {
        let status = Command::new(candidate)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if let Ok(s) = status {
            if s.success() {
                return Ok(candidate.to_string());
            }
        }
    }

    bail!(
        "node runtime not found. install node or bun, or set VIBEFI_NODE_BIN to an executable path"
    )
}

/// Resolve the WalletConnect helper script path.
///
/// Resolution order:
/// 1. `VIBEFI_WC_HELPER_SCRIPT` environment variable
/// 2. Bundled script inside macOS app bundle (`Contents/Resources/walletconnect-helper.mjs`)
/// 3. Source-tree fallback via `CARGO_MANIFEST_DIR` (dev mode)
pub fn resolve_wc_helper_script() -> Result<PathBuf> {
    // 1. Explicit env override
    if let Ok(path) = env::var("VIBEFI_WC_HELPER_SCRIPT") {
        let p = PathBuf::from(&path);
        if p.exists() {
            return Ok(p);
        }
        bail!(
            "VIBEFI_WC_HELPER_SCRIPT is set to {:?} but the file does not exist",
            path
        );
    }

    // 2. Bundled script in app bundle (cargo-packager flattens file resources into Contents/Resources/)
    if let Some(contents) = macos_bundle_contents_dir() {
        let bundled = contents
            .join("Resources")
            .join("walletconnect-helper.mjs");
        if bundled.exists() {
            return Ok(bundled);
        }
    }

    // 3. Dev fallback: source tree
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("walletconnect-helper")
        .join("index.mjs");
    if dev_path.exists() {
        return Ok(dev_path);
    }

    bail!(
        "walletconnect helper script not found. \
         set VIBEFI_WC_HELPER_SCRIPT or ensure the app bundle includes it"
    )
}
