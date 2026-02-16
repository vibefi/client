use anyhow::{Context, Result, anyhow};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::runtime_paths::resolve_bun_binary;
use brk_rolldown::{Bundler, BundlerOptions};
use brk_rolldown_common::bundler_options::{
    InputItem,
    OutputFormat,
    Platform,
    RawMinifyOptions,
};
use brk_rolldown_utils::indexmap::FxIndexMap;

#[derive(Debug, Clone)]
pub struct BundleConfig {
    pub dist_dir: PathBuf,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct BundleManifest {
    pub files: Vec<BundleManifestFile>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct BundleManifestFile {
    pub path: String,
    pub bytes: u64,
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
    "@types/react": "19.2.4",
    "typescript": "5.9.3"
  }
}
"#;

fn write_standard_build_files(bundle_dir: &Path) -> Result<()> {
    fs::write(bundle_dir.join("package.json"), STANDARD_PACKAGE_JSON)?;
    Ok(())
}

pub fn build_bundle(bundle_dir: &Path, dist_dir: &Path) -> Result<()> {
    tracing::info!(
        bundle_dir = %bundle_dir.display(),
        dist_dir = %dist_dir.display(),
        "building bundle"
    );
    write_standard_build_files(bundle_dir)?;
    let bun_bin = resolve_bun_binary().context("resolve bun runtime")?;
    tracing::debug!(
        bun = %bun_bin,
        "resolved bun runtime"
    );

    let node_modules = bundle_dir.join("node_modules");
    if !node_modules.exists() {
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
    }

    fs::create_dir_all(dist_dir).context("create dist dir")?;
    
    tracing::info!(dist_dir = %dist_dir.display(), "running Rolldown build for bundle");
    
    // Find the entry point (main.tsx)
    let entry = bundle_dir.join("src").join("main.tsx");
    if entry.exists() {
        tracing::debug!(entry = %entry.display(), "found entry point");
    } else {
        return Err(anyhow!("No main.tsx entry point found in bundle"));
    }
    
    // Configure Rolldown for bundle build
    let mut define_map = FxIndexMap::default();
    define_map.insert("process.env.NODE_ENV".to_string(), "\"production\"".to_string());
    
    // Define import.meta.env as a complete object matching 
    // Vite's behavior where import.meta.env is always an object
    let env_object = r#"{
  "MODE": "production",
  "DEV": false,
  "PROD": true,
  "SSR": false,
  "BASE_URL": "/",
  "RPC_URL": undefined
}"#;
    define_map.insert("import.meta.env".to_string(), env_object.to_string());
    
    let options = BundlerOptions {
        input: Some(vec![InputItem {
            name: Some("index".to_string()),
            import: entry.to_string_lossy().to_string(),
        }]),
        cwd: Some(bundle_dir.to_path_buf()),
        platform: Some(Platform::Browser),
        format: Some(OutputFormat::Esm),
        dir: Some(dist_dir.to_string_lossy().to_string()),
        minify: Some(RawMinifyOptions::Bool(true)),
        define: Some(define_map),
        ..Default::default()
    };

    // Build with Rolldown
    let mut bundler = Bundler::new(options)
        .context("Failed to create Rolldown bundler")?;
    
    // Use tokio to run async
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to create tokio runtime")?;
    
    let output = runtime.block_on(async {
        bundler.write().await
    });

    match output {
        Ok(bundle_output) => {
            for warning in &bundle_output.warnings {
                tracing::warn!("Rolldown warning: {warning:?}");
            }
            
            // Handle index.html
            let html_src = bundle_dir.join("index.html");
            let html_dest = dist_dir.join("index.html");
            
            if html_src.exists() {
                // Copy existing index.html and update script references
                tracing::debug!("Copying index.html from bundle");
                let html_content = fs::read_to_string(&html_src)
                    .context("Failed to read index.html")?;
                
                // Update script references to point to bundled files
                // Replace common Vite pattern /src/main.tsx with /index.js
                let updated_html = html_content
                    .replace(r#"<script type="module" src="/src/main.tsx"></script>"#, r#"<script type="module" src="/index.js"></script>"#);
                
                fs::write(&html_dest, updated_html)
                    .context("Failed to write index.html to dist")?;
            } else {
                return Err(anyhow!("No index.html found in bundle"));
            }
            
            tracing::info!(dist_dir = %dist_dir.display(), html = %html_dest.display(), "bundle build completed with index.html");
            Ok(())
        }
        Err(e) => {
            tracing::error!("Rolldown build failed: {e:?}");
            Err(anyhow!("Rolldown build failed: {e:?}"))
        }
    }
}
