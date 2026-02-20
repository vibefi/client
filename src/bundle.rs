use anyhow::{Context, Result, anyhow};
use farmfe_compiler::Compiler;
use farmfe_core::HashMap;
use farmfe_core::config::{
    Config as FarmConfig, Mode as FarmMode, OutputConfig as FarmOutputConfig,
    RuntimeConfig as FarmRuntimeConfig, SourcemapConfig as FarmSourcemapConfig,
    config_regex::ConfigRegex,
    persistent_cache::PersistentCacheConfig,
};
use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

use crate::runtime_paths::resolve_bun_binary;

#[derive(Debug, Clone)]
pub struct BundleConfig {
    pub dist_dir: PathBuf,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct BundleManifest {
    pub files: Vec<BundleManifestFile>,
    #[serde(default)]
    pub layout: Option<String>,
    #[serde(default)]
    pub constraints: Option<BundleConstraints>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct BundleManifestFile {
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct BundleConstraints {
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
}

pub fn verify_manifest(bundle_dir: &Path) -> Result<()> {
    tracing::info!(bundle_dir = %bundle_dir.display(), "verifying bundle manifest");
    let manifest_path = bundle_dir.join("manifest.json");
    if !manifest_path.exists() {
        tracing::warn!(
            path = %manifest_path.display(),
            "bundle manifest missing"
        );
        return Err(anyhow!("manifest.json missing in bundle"));
    }
    let content = fs::read_to_string(&manifest_path).context("read manifest.json")?;
    let manifest: BundleManifest = serde_json::from_str(&content).context("parse manifest.json")?;
    tracing::debug!(files = manifest.files.len(), "bundle manifest parsed");
    for entry in manifest.files {
        let file_path = bundle_dir.join(&entry.path);
        if !file_path.exists() {
            tracing::warn!(path = %entry.path, "bundle file listed in manifest is missing");
            return Err(anyhow!("bundle missing file {}", entry.path));
        }
        let meta = fs::metadata(&file_path).context("stat bundle file")?;
        if meta.len() != entry.bytes {
            tracing::warn!(
                path = %entry.path,
                expected = entry.bytes,
                actual = meta.len(),
                "bundle file size mismatch"
            );
            return Err(anyhow!(
                "bundle file size mismatch {} expected {} got {}",
                entry.path,
                entry.bytes,
                meta.len()
            ));
        }
    }
    tracing::info!(bundle_dir = %bundle_dir.display(), "bundle manifest verified");
    Ok(())
}

fn load_manifest(bundle_dir: &Path) -> Result<BundleManifest> {
    let manifest_path = bundle_dir.join("manifest.json");
    let content = fs::read_to_string(&manifest_path).context("read manifest.json")?;
    serde_json::from_str(&content).context("parse manifest.json")
}

fn is_static_html_layout(manifest: &BundleManifest) -> bool {
    if manifest.layout.as_deref() == Some("static-html") {
        return true;
    }
    manifest
        .constraints
        .as_ref()
        .and_then(|c| c.kind.as_deref())
        == Some("static-html")
}

fn validate_static_html_bundle_path(path: &Path) -> Result<()> {
    if path.is_absolute() {
        return Err(anyhow!("static-html bundle file path must be relative"));
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(anyhow!("invalid static-html bundle file path component"));
            }
        }
    }
    Ok(())
}

fn is_allowed_static_html_extension(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("html" | "js" | "json")
    )
}

fn copy_static_html_bundle(
    bundle_dir: &Path,
    dist_dir: &Path,
    manifest: &BundleManifest,
) -> Result<()> {
    if dist_dir.exists() {
        fs::remove_dir_all(dist_dir).context("clear static-html dist dir")?;
    }
    fs::create_dir_all(dist_dir).context("create static-html dist dir")?;

    for entry in &manifest.files {
        let rel = Path::new(&entry.path);
        validate_static_html_bundle_path(rel)
            .with_context(|| format!("invalid static-html bundle path: {}", entry.path))?;
        if !is_allowed_static_html_extension(rel) {
            return Err(anyhow!(
                "static-html build only allows .html/.js/.json files, found: {}",
                entry.path
            ));
        }
        let source = bundle_dir.join(rel);
        let dest = dist_dir.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).context("create static-html output directories")?;
        }
        fs::copy(&source, &dest).with_context(|| {
            format!(
                "copy static-html bundle file {} -> {}",
                source.display(),
                dest.display()
            )
        })?;
    }
    Ok(())
}

const STANDARD_PACKAGE_JSON: &str = r#"{
  "name": "vibefi-dapp",
  "private": true,
  "version": "0.0.1",
  "type": "module",
  "dependencies": {
    "react": "19.2.4",
    "react-dom": "19.2.4",
    "wagmi": "3.4.1",
    "viem": "2.45.0",
    "shadcn": "3.7.0",
    "@tanstack/react-query": "5.90.20"
  },
  "devDependencies": {
    "@farmfe/core": "1.7.11",
    "@farmfe/runtime": "2.0.0-beta.0",
    "@swc/helpers": "0.5.18",
    "@types/react": "19.2.4",
    "typescript": "5.9.3"
  }
}
"#;

const STANDARD_TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ES2022",
    "useDefineForClassFields": true,
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "Bundler",
    "allowImportingTsExtensions": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx",
    "strict": true
  },
  "include": ["src"]
}
"#;

fn write_standard_build_files(bundle_dir: &Path) -> Result<()> {
    fs::write(bundle_dir.join("package.json"), STANDARD_PACKAGE_JSON)?;
    fs::write(bundle_dir.join("tsconfig.json"), STANDARD_TSCONFIG)?;
    Ok(())
}

fn farm_dependencies_available(bundle_dir: &Path) -> bool {
    bundle_dir
        .join("node_modules")
        .join("@farmfe")
        .join("runtime")
        .join("src")
        .join("module-system.ts")
        .is_file()
        && bundle_dir
            .join("node_modules")
            .join("@swc")
            .join("helpers")
            .is_dir()
}

fn ensure_bun_dependencies(bundle_dir: &Path) -> Result<()> {
    let bun_bin = resolve_bun_binary().context("resolve bun runtime")?;
    if farm_dependencies_available(bundle_dir) {
        tracing::debug!(
            bun = %bun_bin,
            "bun runtime resolved and farm dependencies already installed"
        );
        return Ok(());
    }

    tracing::info!("bundle dependencies missing; running bun install");
    let output = Command::new(&bun_bin)
        .arg("install")
        .arg("--no-save")
        .current_dir(bundle_dir)
        .output()
        .with_context(|| format!("bun install failed (runtime: {bun_bin})"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        tracing::warn!(
            status = %output.status,
            bun = %bun_bin,
            %stderr,
            %stdout,
            "bun install failed"
        );
        return Err(anyhow!(
            "bun install failed with status {} (runtime: {bun_bin})\nstdout: {stdout}\nstderr: {stderr}",
            output.status
        ));
    }
    tracing::debug!("bun install completed");
    Ok(())
}

fn build_with_farm(bundle_dir: &Path, dist_dir: &Path) -> Result<()> {
    let runtime_dir = bundle_dir
        .join("node_modules")
        .join("@farmfe")
        .join("runtime");
    let swc_helpers_dir = bundle_dir.join("node_modules").join("@swc").join("helpers");
    if !runtime_dir.is_dir() {
        return Err(anyhow!(
            "farm runtime package not found at {}",
            runtime_dir.display()
        ));
    }
    if !swc_helpers_dir.is_dir() {
        return Err(anyhow!(
            "swc helpers package not found at {}",
            swc_helpers_dir.display()
        ));
    }

    if dist_dir.exists() {
        fs::remove_dir_all(dist_dir).context("clear farm dist dir")?;
    }
    fs::create_dir_all(dist_dir).context("create farm dist dir")?;

    let mut input = HashMap::default();
    input.insert("index".to_string(), "./index.html".to_string());

    let config = FarmConfig {
        input,
        root: bundle_dir.to_string_lossy().into_owned(),
        mode: FarmMode::Production,
        external: vec![
            ConfigRegex::new("^@walletconnect/ethereum-provider(?:/.*)?$"),
            ConfigRegex::new("^@safe-global/safe-apps-provider(?:/.*)?$"),
            ConfigRegex::new("^@safe-global/safe-apps-sdk(?:/.*)?$"),
            ConfigRegex::new("^porto(?:/.*)?$"),
            ConfigRegex::new("^@metamask/sdk(?:/.*)?$"),
            ConfigRegex::new("^@gemini-wallet/core(?:/.*)?$"),
            ConfigRegex::new("^@coinbase/wallet-sdk(?:/.*)?$"),
            ConfigRegex::new("^@base-org/account(?:/.*)?$"),
        ],
        output: Box::new(FarmOutputConfig {
            path: dist_dir.to_string_lossy().into_owned(),
            show_file_size: false,
            ..Default::default()
        }),
        runtime: Box::new(FarmRuntimeConfig {
            path: runtime_dir.to_string_lossy().into_owned(),
            swc_helpers_path: swc_helpers_dir.to_string_lossy().into_owned(),
            ..Default::default()
        }),
        sourcemap: Box::new(FarmSourcemapConfig::Bool(false)),
        lazy_compilation: false,
        progress: false,
        persistent_cache: Box::new(PersistentCacheConfig::Bool(false)),
        ..Default::default()
    };

    tracing::info!(out_dir = %dist_dir.display(), "running farm build for bundle");
    let compiler = Compiler::new(config, vec![]).context("create farm compiler")?;
    compiler.compile().context("farm build failed")?;
    compiler
        .write_resources_to_disk()
        .context("write farm build output")?;
    Ok(())
}

pub fn build_bundle(bundle_dir: &Path, dist_dir: &Path) -> Result<()> {
    tracing::info!(
        bundle_dir = %bundle_dir.display(),
        dist_dir = %dist_dir.display(),
        "building bundle"
    );
    let manifest = load_manifest(bundle_dir)?;
    if is_static_html_layout(&manifest) {
        tracing::info!("static-html layout detected; skipping Farm build");
        copy_static_html_bundle(bundle_dir, dist_dir, &manifest)?;
        tracing::info!(dist_dir = %dist_dir.display(), "static-html bundle copy completed");
        return Ok(());
    }

    write_standard_build_files(bundle_dir)?;
    ensure_bun_dependencies(bundle_dir)?;
    build_with_farm(bundle_dir, dist_dir)?;
    tracing::info!(dist_dir = %dist_dir.display(), "bundle build completed");
    Ok(())
}

pub fn walk_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // Skip generated build files (not part of bundle content)
        if name == "node_modules"
            || name == ".git"
            || name == ".vibefi"
            || name == "package.json"
            || name == "tsconfig.json"
            || name == "bun.lock"
            || name == "bun.lockb"
        {
            continue;
        }
        if entry.file_type()?.is_dir() {
            out.extend(walk_files(&path)?);
        } else if entry.file_type()?.is_file() {
            out.push(path);
        }
    }
    Ok(out)
}
