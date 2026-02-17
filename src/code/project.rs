use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const TEMPLATE_APP_TSX: &str = r#"import { useState } from "react";
import "./App.css";

function App() {
  const [account, setAccount] = useState<string | null>(null);

  async function connect() {
    if (!window.ethereum) return;
    const accounts = await window.ethereum.request({
      method: "eth_requestAccounts",
    });
    setAccount(accounts[0] ?? null);
  }

  return (
    <div className="app">
      <h1>My VibeFi Dapp</h1>
      {account ? (
        <p>Connected: {account.slice(0, 6)}...{account.slice(-4)}</p>
      ) : (
        <button onClick={connect}>Connect Wallet</button>
      )}
    </div>
  );
}

export default App;
"#;

const TEMPLATE_MAIN_TSX: &str = r#"import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
"#;

const TEMPLATE_APP_CSS: &str = r#":root {
  font-family: Inter, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  line-height: 1.5;
  font-weight: 400;
  color: #e2e8f0;
  background: #0f172a;
}

* {
  box-sizing: border-box;
}

body {
  margin: 0;
  min-height: 100vh;
}

#root {
  min-height: 100vh;
}

.app {
  min-height: 100vh;
  display: grid;
  place-content: center;
  gap: 1rem;
  padding: 2rem;
  text-align: center;
}

h1 {
  margin: 0;
  font-size: 2rem;
}

p {
  margin: 0;
  color: #94a3b8;
}

button {
  border: 0;
  border-radius: 0.625rem;
  padding: 0.625rem 1rem;
  font-size: 0.95rem;
  font-weight: 600;
  background: #22c55e;
  color: #052e16;
  cursor: pointer;
}

button:hover {
  background: #4ade80;
}
"#;

const TEMPLATE_INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>VibeFi Dapp</title>
  </head>
  <body>
    <div id="root"></div>
    <script>
      window.addEventListener("error", (event) => {
        window.parent.postMessage(
          {
            type: "vibefi-code-error",
            message: event.message || "Unknown runtime error",
            stack: event.error && event.error.stack ? String(event.error.stack) : "",
          },
          "*"
        );
      });
      window.addEventListener("unhandledrejection", (event) => {
        window.parent.postMessage(
          {
            type: "vibefi-code-error",
            message: String(event.reason || "Unhandled promise rejection"),
          },
          "*"
        );
      });
    </script>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
"#;

const TEMPLATE_TSCONFIG_JSON: &str = r#"{
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

const TEMPLATE_VITE_CONFIG_TS: &str = r#"import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  define: {
    "globalThis.RPC_URL": JSON.stringify(process.env.RPC_URL ?? ""),
  },
  server: {
    strictPort: false,
    hmr: { host: "localhost" },
  },
});
"#;

fn default_base_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        return dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    }

    #[cfg(not(target_os = "windows"))]
    {
        return dirs::data_local_dir()
            .or_else(dirs::data_dir)
            .unwrap_or_else(|| PathBuf::from("."));
    }
}

pub fn resolve_workspace_root() -> PathBuf {
    default_base_data_dir().join("VibeFi").join("code")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEntry {
    pub name: String,
    pub path: String,
    pub last_modified: u64,
}

pub fn create_project(workspace_root: &Path, name: &str) -> Result<PathBuf> {
    let project_name = validate_project_name(name)?;
    std::fs::create_dir_all(workspace_root).with_context(|| {
        format!(
            "failed to create code workspace root {}",
            workspace_root.display()
        )
    })?;

    let project_root = workspace_root.join(&project_name);
    if project_root.exists() {
        bail!("project already exists: {}", project_name);
    }

    std::fs::create_dir_all(project_root.join("src"))
        .with_context(|| format!("failed to create src directory for {}", project_name))?;
    std::fs::create_dir_all(project_root.join("abis"))
        .with_context(|| format!("failed to create abis directory for {}", project_name))?;
    std::fs::create_dir_all(project_root.join("assets"))
        .with_context(|| format!("failed to create assets directory for {}", project_name))?;

    write_scaffold_file(&project_root.join("src/App.tsx"), TEMPLATE_APP_TSX)?;
    write_scaffold_file(&project_root.join("src/main.tsx"), TEMPLATE_MAIN_TSX)?;
    write_scaffold_file(&project_root.join("src/App.css"), TEMPLATE_APP_CSS)?;
    write_scaffold_file(&project_root.join("index.html"), TEMPLATE_INDEX_HTML)?;
    write_scaffold_file(&project_root.join("tsconfig.json"), TEMPLATE_TSCONFIG_JSON)?;
    write_scaffold_file(
        &project_root.join("vite.config.ts"),
        TEMPLATE_VITE_CONFIG_TS,
    )?;

    write_scaffold_file(
        &project_root.join("manifest.json"),
        &render_manifest_json(&project_name)?,
    )?;
    write_scaffold_file(
        &project_root.join("package.json"),
        &render_package_json(&project_name)?,
    )?;
    write_scaffold_file(&project_root.join("addresses.json"), "{}\n")?;

    project_root.canonicalize().with_context(|| {
        format!(
            "failed to canonicalize created project path {}",
            project_root.display()
        )
    })
}

pub fn list_projects(workspace_root: &Path) -> Result<Vec<ProjectEntry>> {
    std::fs::create_dir_all(workspace_root).with_context(|| {
        format!(
            "failed to create code workspace root {}",
            workspace_root.display()
        )
    })?;

    let mut projects = Vec::new();
    for entry in std::fs::read_dir(workspace_root)
        .with_context(|| format!("failed to read workspace root {}", workspace_root.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }

        let path = entry.path();
        if validate_project_root(&path).is_err() {
            continue;
        }

        let canonical_path = path
            .canonicalize()
            .with_context(|| format!("failed to resolve project path {}", path.display()))?;
        let metadata = entry.metadata()?;
        let last_modified = metadata
            .modified()
            .ok()
            .and_then(|ts| ts.duration_since(UNIX_EPOCH).ok())
            .map(|duration| {
                let ms = duration.as_millis();
                u64::try_from(ms).unwrap_or(u64::MAX)
            })
            .unwrap_or(0);

        projects.push(ProjectEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            path: canonical_path.to_string_lossy().into_owned(),
            last_modified,
        });
    }

    projects.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(projects)
}

pub fn resolve_open_project_path(workspace_root: &Path, requested_path: &str) -> Result<PathBuf> {
    let raw = requested_path.trim();
    if raw.is_empty() {
        bail!("project path must not be empty");
    }

    let input = Path::new(raw);
    let candidate = if input.is_absolute() {
        input.to_path_buf()
    } else {
        workspace_root.join(input)
    };

    let project_root = candidate
        .canonicalize()
        .with_context(|| format!("project path does not exist: {}", candidate.display()))?;
    validate_project_root(&project_root)?;
    Ok(project_root)
}

pub fn fork_project_from_source(
    workspace_root: &Path,
    source_root: &Path,
    preferred_name: Option<&str>,
) -> Result<PathBuf> {
    std::fs::create_dir_all(workspace_root).with_context(|| {
        format!(
            "failed to create code workspace root {}",
            workspace_root.display()
        )
    })?;

    let source_root = source_root
        .canonicalize()
        .with_context(|| format!("source path does not exist: {}", source_root.display()))?;
    if !source_root.is_dir() {
        bail!(
            "source not available for this dapp: {} is not a directory",
            source_root.display()
        );
    }

    let fallback_name = source_root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("dapp");
    let base_name = sanitize_fork_name(preferred_name.unwrap_or(fallback_name));
    let target_root = allocate_fork_project_root(workspace_root, &base_name)?;

    if let Err(err) = (|| -> Result<()> {
        copy_source_tree(&source_root, &target_root)?;
        validate_project_root(&target_root)?;
        Ok(())
    })() {
        let _ = std::fs::remove_dir_all(&target_root);
        return Err(err);
    }

    target_root.canonicalize().with_context(|| {
        format!(
            "failed to canonicalize forked project path {}",
            target_root.display()
        )
    })
}

pub fn validate_project_root(project_root: &Path) -> Result<()> {
    if !project_root.is_dir() {
        bail!(
            "project path is not a directory: {}",
            project_root.display()
        );
    }

    let package_json = project_root.join("package.json");
    if !package_json.is_file() {
        bail!(
            "invalid VibeFi project: missing package.json at {}",
            package_json.display()
        );
    }

    let manifest_json = project_root.join("manifest.json");
    if !manifest_json.is_file() {
        bail!(
            "invalid VibeFi project: missing manifest.json at {}",
            manifest_json.display()
        );
    }

    Ok(())
}

fn sanitize_fork_name(raw: &str) -> String {
    let trimmed = raw.trim();
    let mut out = String::with_capacity(trimmed.len());
    let mut last_dash = false;
    for ch in trimmed.chars() {
        let mapped = if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            ch
        } else {
            '-'
        };
        if mapped == '-' {
            if last_dash {
                continue;
            }
            last_dash = true;
            out.push('-');
        } else {
            last_dash = false;
            out.push(mapped);
        }
    }

    let out = out.trim_matches('-');
    if out.is_empty() {
        "dapp".to_string()
    } else {
        out.to_string()
    }
}

fn allocate_fork_project_root(workspace_root: &Path, base_name: &str) -> Result<PathBuf> {
    let prefix = format!("{}-fork", sanitize_fork_name(base_name));
    for index in 1..10_000u32 {
        let candidate_name = if index == 1 {
            prefix.clone()
        } else {
            format!("{}-{}", prefix, index)
        };
        let candidate = workspace_root.join(candidate_name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    bail!("failed to allocate a unique fork project directory name");
}

fn should_skip_dir_name(name: &str) -> bool {
    name == "node_modules" || name == ".vibefi" || name == "dist"
}

fn copy_source_tree(source_root: &Path, target_root: &Path) -> Result<()> {
    std::fs::create_dir_all(target_root)
        .with_context(|| format!("failed to create {}", target_root.display()))?;

    let entries = std::fs::read_dir(source_root)
        .with_context(|| format!("failed to read {}", source_root.display()))?;
    for entry in entries {
        let entry = entry?;
        let file_name = entry.file_name();
        let file_name_string = file_name.to_string_lossy().into_owned();
        let source_path = entry.path();
        let target_path = target_root.join(&file_name);
        let file_type = entry.file_type()?;

        if source_path == target_root {
            continue;
        }

        if file_type.is_symlink() {
            continue;
        }

        if file_type.is_dir() {
            if should_skip_dir_name(&file_name_string) {
                continue;
            }
            copy_source_tree(&source_path, &target_path)?;
            continue;
        }

        if file_type.is_file() {
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            std::fs::copy(&source_path, &target_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source_path.display(),
                    target_path.display()
                )
            })?;
        }
    }

    Ok(())
}

fn validate_project_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        bail!("project name must not be empty");
    }
    if trimmed == "." || trimmed == ".." {
        bail!("invalid project name: {}", trimmed);
    }
    if trimmed.starts_with('.') {
        bail!("project name must not start with '.'");
    }
    if trimmed.len() > 64 {
        bail!("project name is too long (max 64 characters)");
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        bail!("project name must not contain path separators");
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        bail!("project name must use only letters, numbers, '-' or '_'");
    }
    Ok(trimmed.to_owned())
}

fn write_scaffold_file(path: &Path, contents: &str) -> Result<()> {
    std::fs::write(path, contents)
        .with_context(|| format!("failed to write scaffold file {}", path.display()))
}

fn render_manifest_json(project_name: &str) -> Result<String> {
    let value = serde_json::json!({
        "name": project_name,
        "version": "0.1.0",
    });
    let mut out = serde_json::to_string_pretty(&value)
        .context("failed to serialize scaffold manifest.json")?;
    out.push('\n');
    Ok(out)
}

fn render_package_json(project_name: &str) -> Result<String> {
    let value = serde_json::json!({
        "name": project_name,
        "private": true,
        "version": "0.1.0",
        "type": "module",
        "scripts": {
            "dev": "vite",
            "build": "vite build",
            "preview": "vite preview"
        },
        "dependencies": {
            "react": "19.2.4",
            "react-dom": "19.2.4",
            "wagmi": "3.4.1",
            "viem": "2.45.0",
            "@tanstack/react-query": "5.90.20"
        },
        "devDependencies": {
            "@vitejs/plugin-react": "5.1.2",
            "@types/react": "19.2.4",
            "@types/react-dom": "19.2.2",
            "typescript": "5.9.3",
            "vite": "7.2.4"
        }
    });
    let mut out = serde_json::to_string_pretty(&value)
        .context("failed to serialize scaffold package.json")?;
    out.push('\n');
    Ok(out)
}
