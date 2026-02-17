use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use std::fs;
use std::path::{Component, Path, PathBuf};

const ALLOWED_FILE_EXTENSIONS: &[&str] = &["ts", "tsx", "css", "json", "html", "webp"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteFileKind {
    Create,
    Modify,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<FileEntry>>,
}

pub fn resolve_project_root(project_path: &str) -> Result<PathBuf> {
    let canonical = PathBuf::from(project_path)
        .canonicalize()
        .with_context(|| format!("project path does not exist: {project_path}"))?;
    if !canonical.is_dir() {
        bail!("project path is not a directory: {}", canonical.display());
    }
    Ok(canonical)
}

pub fn list_files(project_root: &Path) -> Result<Vec<FileEntry>> {
    list_dir(project_root, project_root)
}

pub fn read_file(project_root: &Path, relative_path: &str) -> Result<String> {
    let path = validate_relative_path(project_root, relative_path)?;
    if !path.is_file() {
        bail!("file not found: {}", relative_path);
    }
    fs::read_to_string(&path).with_context(|| format!("failed to read {}", relative_path))
}

pub fn write_file(project_root: &Path, relative_path: &str, content: &str) -> Result<WriteFileKind> {
    let path = validate_relative_path(project_root, relative_path)?;
    validate_write_extension(&path)?;
    let existed = path.is_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create parent directories for {}", relative_path)
        })?;
    }
    fs::write(&path, content).with_context(|| format!("failed to write {}", relative_path))?;
    Ok(if existed {
        WriteFileKind::Modify
    } else {
        WriteFileKind::Create
    })
}

pub fn delete_file(project_root: &Path, relative_path: &str) -> Result<()> {
    let path = validate_relative_path(project_root, relative_path)?;
    if !path.exists() {
        bail!("file not found: {}", relative_path);
    }
    if path.is_dir() {
        bail!("expected file path, found directory: {}", relative_path);
    }
    fs::remove_file(&path).with_context(|| format!("failed to delete {}", relative_path))?;
    Ok(())
}

pub fn create_dir(project_root: &Path, relative_path: &str) -> Result<()> {
    let path = validate_relative_path(project_root, relative_path)?;
    fs::create_dir_all(&path)
        .with_context(|| format!("failed to create directory {}", relative_path))?;
    Ok(())
}

pub fn validate_relative_path(project_root: &Path, relative_path: &str) -> Result<PathBuf> {
    let normalized = normalize_relative_path(relative_path)?;
    if normalized.as_os_str().is_empty() {
        bail!("path must not be empty");
    }
    if contains_blocked_component(&normalized) {
        bail!("blocked path segment in {relative_path}");
    }

    let root_canonical = project_root
        .canonicalize()
        .with_context(|| format!("failed to resolve project root {}", project_root.display()))?;
    let candidate = project_root.join(normalized);
    let candidate_anchor = canonicalize_anchor(&candidate)?;
    if !candidate_anchor.starts_with(&root_canonical) {
        bail!("path traversal attempt: {}", relative_path);
    }

    Ok(candidate)
}

fn normalize_relative_path(relative_path: &str) -> Result<PathBuf> {
    let path = Path::new(relative_path);
    if path.is_absolute() {
        bail!("absolute paths are not allowed: {}", relative_path);
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(segment) => normalized.push(segment),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(anyhow!("path traversal attempt: {}", relative_path));
            }
        }
    }

    Ok(normalized)
}

fn contains_blocked_component(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(part) => {
            let value = part.to_string_lossy();
            value == "node_modules" || value == ".vibefi" || value.starts_with('.')
        }
        _ => false,
    })
}

fn validate_write_extension(path: &Path) -> Result<()> {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .ok_or_else(|| anyhow!("file extension is required"))?;

    if !ALLOWED_FILE_EXTENSIONS
        .iter()
        .any(|allowed| allowed == &ext.as_str())
    {
        bail!("disallowed file extension: .{}", ext);
    }

    Ok(())
}

fn list_dir(project_root: &Path, current_dir: &Path) -> Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    for dir_entry in fs::read_dir(current_dir)
        .with_context(|| format!("failed to read directory {}", current_dir.display()))?
    {
        let dir_entry = dir_entry?;
        let file_name = dir_entry.file_name();
        let name = file_name.to_string_lossy().to_string();
        if name == "node_modules" || name == ".vibefi" || name.starts_with('.') {
            continue;
        }

        let path = dir_entry.path();
        let file_type = dir_entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        let metadata = dir_entry.metadata()?;
        let relative = path
            .strip_prefix(project_root)
            .with_context(|| format!("failed to strip project root from {}", path.display()))?;
        let relative_path = relative
            .components()
            .filter_map(|component| match component {
                Component::Normal(part) => Some(part.to_string_lossy().to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/");

        if metadata.is_dir() {
            let children = list_dir(project_root, &path)?;
            entries.push(FileEntry {
                name,
                path: relative_path,
                is_dir: true,
                size: None,
                children: Some(children),
            });
        } else if metadata.is_file() {
            entries.push(FileEntry {
                name,
                path: relative_path,
                is_dir: false,
                size: Some(metadata.len()),
                children: None,
            });
        }
    }

    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

fn canonicalize_anchor(candidate: &Path) -> Result<PathBuf> {
    if candidate.exists() {
        return candidate
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {}", candidate.display()));
    }

    let mut ancestor = candidate;
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| anyhow!("failed to find existing parent for {}", candidate.display()))?;
    }

    let ancestor_canonical = ancestor
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", ancestor.display()))?;
    let suffix = candidate
        .strip_prefix(ancestor)
        .with_context(|| format!("failed to compute path suffix for {}", candidate.display()))?;
    Ok(ancestor_canonical.join(suffix))
}
