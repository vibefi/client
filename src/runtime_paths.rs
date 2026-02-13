use anyhow::{Result, bail};
use std::env;
use std::path::{Path, PathBuf};
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

/// Returns the install prefix for Linux packaged layouts, e.g. `/usr`
/// from an executable path like `/usr/bin/vibefi`.
fn linux_install_prefix_dir() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let bin_dir = exe.parent()?;
    if bin_dir.file_name()?.to_str()? != "bin" {
        return None;
    }
    Some(bin_dir.parent()?.to_path_buf())
}

fn command_version_ok(bin: &Path) -> bool {
    Command::new(bin)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn probe_working_path_binary(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() && command_version_ok(&candidate) {
            return Some(candidate);
        }
        // On Windows, executables typically have a .exe extension.
        #[cfg(windows)]
        {
            let candidate_exe = dir.join(format!("{name}.exe"));
            if candidate_exe.is_file() && command_version_ok(&candidate_exe) {
                return Some(candidate_exe);
            }
        }
    }
    None
}

/// Resolve a Bun runtime binary for JSX bundle builds.
///
/// Resolution order:
/// 1. `VIBEFI_BUN_BIN` environment variable (if it points to a working binary)
/// 2. Bundled binary inside macOS app bundle (`Contents/MacOS/bun`)
/// 3. Bundled binary in Linux package layouts (`<prefix>/bin/bun`)
/// 4. PATH probe for a working `bun` binary
pub fn resolve_bun_binary() -> Result<String> {
    // 1. Explicit env override
    if let Ok(bin) = env::var("VIBEFI_BUN_BIN") {
        let trimmed = bin.trim();
        if !trimmed.is_empty() {
            let p = PathBuf::from(trimmed);
            if command_version_ok(&p) {
                return Ok(trimmed.to_string());
            }
            bail!(
                "VIBEFI_BUN_BIN is set to {:?} but `--version` failed",
                trimmed
            );
        }
    }

    // 2. Bundled binary in app bundle
    if let Some(contents) = macos_bundle_contents_dir() {
        let bundled = contents.join("MacOS").join("bun");
        if bundled.exists() && command_version_ok(&bundled) {
            return Ok(bundled.to_string_lossy().into_owned());
        }
    }

    // 3. Bundled binary in Linux package layouts (deb/appimage)
    if let Some(prefix) = linux_install_prefix_dir() {
        let bundled = prefix.join("bin").join("bun");
        if bundled.exists() && command_version_ok(&bundled) {
            return Ok(bundled.to_string_lossy().into_owned());
        }
    }

    // 4. PATH fallback (dev mode and package fallback)
    if let Some(bun) = probe_working_path_binary("bun") {
        return Ok(bun.to_string_lossy().into_owned());
    }

    bail!("bun runtime not found. install bun or set VIBEFI_BUN_BIN to a working executable path")
}

/// Resolve the Node/Bun runtime binary.
///
/// Resolution order:
/// 1. `VIBEFI_NODE_BIN` environment variable
/// 2. Bundled binary inside macOS app bundle (`Contents/MacOS/bun`)
/// 3. Bundled binary in Linux package layouts (`<prefix>/bin/bun`)
/// 4. PATH probe (`bun`, then `node`) for development
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

    // 3. Bundled binary in Linux package layouts (deb/appimage)
    if let Some(prefix) = linux_install_prefix_dir() {
        let bundled = prefix.join("bin").join("bun");
        if bundled.exists() {
            return Ok(bundled.to_string_lossy().into_owned());
        }
    }

    // 4. PATH fallback (dev mode)
    if let Some(bun) = probe_working_path_binary("bun") {
        return Ok(bun.to_string_lossy().into_owned());
    }
    if let Some(node) = probe_working_path_binary("node") {
        return Ok(node.to_string_lossy().into_owned());
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
/// 3. Bundled script in Linux package layouts (`<prefix>/lib/<pkg>/walletconnect-helper.mjs`)
/// 4. Source-tree fallback via `CARGO_MANIFEST_DIR` (dev mode)
pub fn resolve_wc_helper_script() -> Result<PathBuf> {
    // 1. Explicit env override
    if let Ok(path) = env::var("VIBEFI_WC_HELPER_SCRIPT") {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            bail!("VIBEFI_WC_HELPER_SCRIPT is set but empty or whitespace");
        }
        let p = PathBuf::from(trimmed);
        if p.is_file() {
            return Ok(p);
        }
        bail!(
            "VIBEFI_WC_HELPER_SCRIPT is set to {:?} but the file does not exist or is not a regular file",
            path
        );
    }

    // 2. Bundled script in app bundle (cargo-packager flattens file resources into Contents/Resources/)
    if let Some(contents) = macos_bundle_contents_dir() {
        let bundled = contents.join("Resources").join("walletconnect-helper.mjs");
        if bundled.exists() {
            return Ok(bundled);
        }
    }

    // 3. Bundled script in Linux package layouts (deb/appimage)
    if let Some(prefix) = linux_install_prefix_dir() {
        let bundled = prefix
            .join("lib")
            .join(env!("CARGO_PKG_NAME"))
            .join("walletconnect-helper.mjs");
        if bundled.exists() {
            return Ok(bundled);
        }
    }

    // 4. Dev fallback: source tree
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

/// Resolve the IPFS helper script path.
///
/// Resolution order:
/// 1. `VIBEFI_IPFS_HELPER_SCRIPT` environment variable
/// 2. Bundled script inside macOS app bundle (`Contents/Resources/ipfs-helper/index.mjs`)
/// 3. Bundled script in Linux package layouts (`<prefix>/lib/<pkg>/ipfs-helper/index.mjs`)
/// 4. Source-tree fallback via `CARGO_MANIFEST_DIR` (dev mode)
pub fn resolve_ipfs_helper_script() -> Result<PathBuf> {
    // 1. Explicit env override
    if let Ok(path) = env::var("VIBEFI_IPFS_HELPER_SCRIPT") {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            bail!("VIBEFI_IPFS_HELPER_SCRIPT is set but empty or whitespace");
        }
        let p = PathBuf::from(trimmed);
        if p.is_file() {
            return Ok(p);
        }
        bail!(
            "VIBEFI_IPFS_HELPER_SCRIPT is set to {:?} but the file does not exist or is not a regular file",
            path
        );
    }

    // 2. Bundled script in app bundle (cargo-packager flattens file resources into Contents/Resources/)
    if let Some(contents) = macos_bundle_contents_dir() {
        let bundled_file = contents.join("Resources").join("ipfs-helper.mjs");
        if bundled_file.exists() {
            return Ok(bundled_file);
        }
        let bundled_dir = contents
            .join("Resources")
            .join("ipfs-helper")
            .join("index.mjs");
        if bundled_dir.exists() {
            return Ok(bundled_dir);
        }
    }

    // 3. Bundled script in Linux package layouts (deb/appimage)
    if let Some(prefix) = linux_install_prefix_dir() {
        let bundled_file = prefix
            .join("lib")
            .join(env!("CARGO_PKG_NAME"))
            .join("ipfs-helper.mjs");
        if bundled_file.exists() {
            return Ok(bundled_file);
        }
        let bundled_dir = prefix
            .join("lib")
            .join(env!("CARGO_PKG_NAME"))
            .join("ipfs-helper")
            .join("index.mjs");
        if bundled_dir.exists() {
            return Ok(bundled_dir);
        }
    }

    // 4. Dev fallback: source tree
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("ipfs-helper")
        .join("index.mjs");
    if dev_path.exists() {
        return Ok(dev_path);
    }

    bail!(
        "ipfs helper script not found. \
         set VIBEFI_IPFS_HELPER_SCRIPT or ensure the app bundle includes it"
    )
}
