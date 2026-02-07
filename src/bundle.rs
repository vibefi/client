use anyhow::{anyhow, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone)]
pub struct BundleConfig {
    pub source_dir: PathBuf,
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
    let manifest_path = bundle_dir.join("manifest.json");
    if !manifest_path.exists() {
        return Err(anyhow!("manifest.json missing in bundle"));
    }
    let content = fs::read_to_string(&manifest_path).context("read manifest.json")?;
    let manifest: BundleManifest = serde_json::from_str(&content).context("parse manifest.json")?;
    for entry in manifest.files {
        let file_path = bundle_dir.join(&entry.path);
        if !file_path.exists() {
            return Err(anyhow!("bundle missing file {}", entry.path));
        }
        let meta = fs::metadata(&file_path).context("stat bundle file")?;
        if meta.len() != entry.bytes {
            return Err(anyhow!(
                "bundle file size mismatch {} expected {} got {}",
                entry.path,
                entry.bytes,
                meta.len()
            ));
        }
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
    "@vitejs/plugin-react": "5.1.2",
    "@types/react": "19.2.4",
    "typescript": "5.9.3",
    "vite": "7.2.4"
  }
}
"#;

const STANDARD_VITE_CONFIG: &str = r#"import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
});
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
    fs::write(bundle_dir.join("vite.config.ts"), STANDARD_VITE_CONFIG)?;
    fs::write(bundle_dir.join("tsconfig.json"), STANDARD_TSCONFIG)?;
    Ok(())
}

pub fn build_bundle(bundle_dir: &Path, dist_dir: &Path) -> Result<()> {
    write_standard_build_files(bundle_dir)?;

    let node_modules = bundle_dir.join("node_modules");
    if !node_modules.exists() {
        let status = Command::new("bun")
            .arg("install")
            .arg("--no-save")
            .current_dir(bundle_dir)
            .status()
            .context("bun install failed")?;
        if !status.success() {
            return Err(anyhow!("bun install failed"));
        }
    }

    fs::create_dir_all(dist_dir).context("create dist dir")?;
    // Use relative path from bundle_dir for vite's outDir since vite runs in bundle_dir
    let relative_dist = PathBuf::from(".vibefi").join("dist");
    let status = Command::new("bun")
        .arg("x")
        .arg("vite")
        .arg("build")
        .arg("--emptyOutDir")
        .arg("--outDir")
        .arg(&relative_dist)
        .current_dir(bundle_dir)
        .status()
        .context("bun vite build failed")?;
    if !status.success() {
        return Err(anyhow!("bun vite build failed"));
    }
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
            || name == "vite.config.ts"
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
