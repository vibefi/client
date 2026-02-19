use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::{Component, Path, PathBuf};

const ALLOWED_SRC_EXTENSIONS: &[&str] = &["ts", "tsx", "css"];
const ALLOWED_ABIS_EXTENSIONS: &[&str] = &["json"];
const ALLOWED_ASSETS_EXTENSIONS: &[&str] = &["webp"];
const ALLOWED_IPFS_AS_KINDS: &[&str] = &["json", "text", "snippet", "image"];
const SECURITY_RULES: &[(&str, &str, ValidationSeverity)] = &[
    (
        "eval(",
        "Avoid `eval(` in project code.",
        ValidationSeverity::Warning,
    ),
    (
        "new Function(",
        "Avoid `new Function(` in project code.",
        ValidationSeverity::Warning,
    ),
    (
        "innerHTML",
        "Avoid `innerHTML` for untrusted content.",
        ValidationSeverity::Warning,
    ),
    (
        "dangerouslySetInnerHTML",
        "Avoid `dangerouslySetInnerHTML` for untrusted content.",
        ValidationSeverity::Warning,
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ValidationSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationError {
    pub severity: ValidationSeverity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    pub message: String,
    pub rule: String,
}

impl ValidationError {
    fn new(
        severity: ValidationSeverity,
        file: Option<&str>,
        line: Option<usize>,
        message: impl Into<String>,
        rule: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            file: file.map(ToOwned::to_owned),
            line,
            message: message.into(),
            rule: rule.into(),
        }
    }

    fn error(
        file: Option<&str>,
        line: Option<usize>,
        message: impl Into<String>,
        rule: impl Into<String>,
    ) -> Self {
        Self::new(ValidationSeverity::Error, file, line, message, rule)
    }

    fn warning(
        file: Option<&str>,
        line: Option<usize>,
        message: impl Into<String>,
        rule: impl Into<String>,
    ) -> Self {
        Self::new(ValidationSeverity::Warning, file, line, message, rule)
    }
}

#[derive(Debug, Clone)]
struct ProjectFile {
    relative_path: String,
    absolute_path: PathBuf,
}

pub fn validate_project(project_root: &Path) -> Result<Vec<ValidationError>> {
    let mut files = Vec::new();
    collect_project_files(project_root, project_root, &mut files)?;

    let mut errors = Vec::new();
    validate_file_types(&files, &mut errors);
    validate_package_json(project_root, &mut errors)?;
    validate_manifest_json(project_root, &mut errors)?;
    validate_security_patterns(&files, &mut errors)?;

    errors.sort_by(|left, right| {
        severity_rank(left.severity)
            .cmp(&severity_rank(right.severity))
            .then(left.file.cmp(&right.file))
            .then(left.line.cmp(&right.line))
            .then(left.rule.cmp(&right.rule))
            .then(left.message.cmp(&right.message))
    });
    Ok(errors)
}

pub fn is_valid(errors: &[ValidationError]) -> bool {
    !errors
        .iter()
        .any(|error| error.severity == ValidationSeverity::Error)
}

fn severity_rank(severity: ValidationSeverity) -> u8 {
    match severity {
        ValidationSeverity::Error => 0,
        ValidationSeverity::Warning => 1,
    }
}

fn collect_project_files(
    project_root: &Path,
    current_dir: &Path,
    files: &mut Vec<ProjectFile>,
) -> Result<()> {
    for entry in fs::read_dir(current_dir)
        .with_context(|| format!("failed to read directory {}", current_dir.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }

        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if file_type.is_dir() {
            if should_skip_directory(&file_name) {
                continue;
            }
            collect_project_files(project_root, &entry.path(), files)?;
            continue;
        }

        if !file_type.is_file() {
            continue;
        }

        let relative_path = to_relative_path(project_root, &entry.path())?;
        files.push(ProjectFile {
            relative_path,
            absolute_path: entry.path(),
        });
    }

    Ok(())
}

fn should_skip_directory(name: &str) -> bool {
    name == "node_modules" || name == ".vibefi" || name == "dist" || name.starts_with('.')
}

fn to_relative_path(project_root: &Path, path: &Path) -> Result<String> {
    let relative = path
        .strip_prefix(project_root)
        .with_context(|| format!("failed to strip {}", project_root.display()))?;
    let parts = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    Ok(parts.join("/"))
}

fn validate_file_types(files: &[ProjectFile], errors: &mut Vec<ValidationError>) {
    for file in files {
        if file.relative_path.starts_with("src/")
            && !has_allowed_extension(&file.relative_path, ALLOWED_SRC_EXTENSIONS)
        {
            errors.push(ValidationError::error(
                Some(file.relative_path.as_str()),
                None,
                "Files under src/ must use .ts, .tsx, or .css extensions.",
                "invalid-file-type",
            ));
        } else if file.relative_path.starts_with("abis/")
            && !has_allowed_extension(&file.relative_path, ALLOWED_ABIS_EXTENSIONS)
        {
            errors.push(ValidationError::error(
                Some(file.relative_path.as_str()),
                None,
                "Files under abis/ must use .json extension.",
                "invalid-file-type",
            ));
        } else if file.relative_path.starts_with("assets/")
            && !has_allowed_extension(&file.relative_path, ALLOWED_ASSETS_EXTENSIONS)
        {
            errors.push(ValidationError::error(
                Some(file.relative_path.as_str()),
                None,
                "Files under assets/ must use .webp extension.",
                "invalid-file-type",
            ));
        }
    }
}

fn has_allowed_extension(path: &str, allowed: &[&str]) -> bool {
    let extension = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    match extension {
        Some(extension) => allowed.iter().any(|item| *item == extension),
        None => false,
    }
}

fn validate_package_json(project_root: &Path, errors: &mut Vec<ValidationError>) -> Result<()> {
    let package_json_path = project_root.join("package.json");
    let raw = fs::read_to_string(&package_json_path)
        .with_context(|| format!("failed to read {}", package_json_path.display()))?;
    let value: Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(err) => {
            errors.push(ValidationError::error(
                Some("package.json"),
                Some(err.line()),
                format!("Invalid JSON in package.json: {}", err),
                "invalid-package-json",
            ));
            return Ok(());
        }
    };

    let Some(root) = value.as_object() else {
        errors.push(ValidationError::error(
            Some("package.json"),
            None,
            "package.json must be a JSON object.",
            "invalid-package-json",
        ));
        return Ok(());
    };

    for section in ["dependencies", "devDependencies"] {
        let Some(section_value) = root.get(section) else {
            continue;
        };
        let Some(section_object) = section_value.as_object() else {
            errors.push(ValidationError::error(
                Some("package.json"),
                None,
                format!("{section} must be an object of package names to versions."),
                "invalid-package-json",
            ));
            continue;
        };

        for package_name in section_object.keys() {
            if !is_allowed_package(package_name) {
                errors.push(ValidationError::error(
                    Some("package.json"),
                    None,
                    format!("Package `{}` is not in the approved allowlist.", package_name),
                    "disallowed-package",
                ));
            }
        }
    }

    Ok(())
}

fn is_allowed_package(name: &str) -> bool {
    matches!(
        name,
        "react"
            | "react-dom"
            | "typescript"
            | "vite"
            | "@tanstack/react-query"
            | "wagmi"
            | "viem"
            | "shadcn"
            | "@types/react"
            | "@types/react-dom"
            | "@vitejs/plugin-react"
    )
}

fn validate_manifest_json(project_root: &Path, errors: &mut Vec<ValidationError>) -> Result<()> {
    let manifest_path = project_root.join("manifest.json");
    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let value: Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(err) => {
            errors.push(ValidationError::error(
                Some("manifest.json"),
                Some(err.line()),
                format!("Invalid JSON in manifest.json: {}", err),
                "invalid-manifest",
            ));
            return Ok(());
        }
    };

    let Some(root) = value.as_object() else {
        errors.push(ValidationError::error(
            Some("manifest.json"),
            None,
            "manifest.json must be a JSON object.",
            "invalid-manifest",
        ));
        return Ok(());
    };

    let Some(capabilities_value) = root.get("capabilities") else {
        return Ok(());
    };
    let Some(capabilities) = capabilities_value.as_object() else {
        errors.push(ValidationError::error(
            Some("manifest.json"),
            None,
            "`capabilities` must be a JSON object.",
            "invalid-manifest",
        ));
        return Ok(());
    };

    let Some(ipfs_value) = capabilities.get("ipfs") else {
        return Ok(());
    };
    let Some(ipfs) = ipfs_value.as_object() else {
        errors.push(ValidationError::error(
            Some("manifest.json"),
            None,
            "`capabilities.ipfs` must be a JSON object.",
            "invalid-manifest",
        ));
        return Ok(());
    };

    let Some(allow_value) = ipfs.get("allow") else {
        return Ok(());
    };
    let Some(allow_rules) = allow_value.as_array() else {
        errors.push(ValidationError::error(
            Some("manifest.json"),
            None,
            "`capabilities.ipfs.allow` must be an array.",
            "invalid-manifest",
        ));
        return Ok(());
    };

    for (index, rule_value) in allow_rules.iter().enumerate() {
        let Some(rule) = rule_value.as_object() else {
            errors.push(ValidationError::error(
                Some("manifest.json"),
                None,
                format!("capabilities.ipfs.allow[{index}] must be an object."),
                "invalid-manifest",
            ));
            continue;
        };

        match rule.get("cid") {
            Some(Value::String(cid)) if cid.trim().is_empty() => {
                errors.push(ValidationError::error(
                    Some("manifest.json"),
                    None,
                    format!("capabilities.ipfs.allow[{index}].cid must not be empty."),
                    "invalid-manifest",
                ));
            }
            Some(Value::String(_)) | None => {}
            Some(_) => {
                errors.push(ValidationError::error(
                    Some("manifest.json"),
                    None,
                    format!("capabilities.ipfs.allow[{index}].cid must be a string."),
                    "invalid-manifest",
                ));
            }
        }

        let Some(paths_value) = rule.get("paths") else {
            errors.push(ValidationError::error(
                Some("manifest.json"),
                None,
                format!("capabilities.ipfs.allow[{index}].paths is required."),
                "invalid-manifest",
            ));
            continue;
        };
        let Some(paths) = paths_value.as_array() else {
            errors.push(ValidationError::error(
                Some("manifest.json"),
                None,
                format!("capabilities.ipfs.allow[{index}].paths must be an array."),
                "invalid-manifest",
            ));
            continue;
        };
        if paths.is_empty() {
            errors.push(ValidationError::error(
                Some("manifest.json"),
                None,
                format!("capabilities.ipfs.allow[{index}].paths must not be empty."),
                "invalid-manifest",
            ));
        }
        for path_value in paths {
            match path_value {
                Value::String(path) if !path.trim().is_empty() => {}
                _ => errors.push(ValidationError::error(
                    Some("manifest.json"),
                    None,
                    format!(
                        "capabilities.ipfs.allow[{index}].paths entries must be non-empty strings."
                    ),
                    "invalid-manifest",
                )),
            }
        }

        let Some(as_value) = rule.get("as") else {
            errors.push(ValidationError::error(
                Some("manifest.json"),
                None,
                format!("capabilities.ipfs.allow[{index}].as is required."),
                "invalid-manifest",
            ));
            continue;
        };
        let Some(as_entries) = as_value.as_array() else {
            errors.push(ValidationError::error(
                Some("manifest.json"),
                None,
                format!("capabilities.ipfs.allow[{index}].as must be an array."),
                "invalid-manifest",
            ));
            continue;
        };
        if as_entries.is_empty() {
            errors.push(ValidationError::error(
                Some("manifest.json"),
                None,
                format!("capabilities.ipfs.allow[{index}].as must not be empty."),
                "invalid-manifest",
            ));
        }
        for as_entry in as_entries {
            match as_entry {
                Value::String(value) => {
                    let value = value.trim().to_ascii_lowercase();
                    if !ALLOWED_IPFS_AS_KINDS.iter().any(|allowed| allowed == &value) {
                        errors.push(ValidationError::error(
                            Some("manifest.json"),
                            None,
                            format!(
                                "capabilities.ipfs.allow[{index}].as contains unsupported value `{}`.",
                                value
                            ),
                            "invalid-manifest",
                        ));
                    }
                }
                _ => errors.push(ValidationError::error(
                    Some("manifest.json"),
                    None,
                    format!("capabilities.ipfs.allow[{index}].as entries must be strings."),
                    "invalid-manifest",
                )),
            }
        }

        if let Some(max_bytes_value) = rule.get("maxBytes") {
            match max_bytes_value.as_u64() {
                Some(value) if value > 0 => {}
                _ => errors.push(ValidationError::error(
                    Some("manifest.json"),
                    None,
                    format!(
                        "capabilities.ipfs.allow[{index}].maxBytes must be a positive integer when present."
                    ),
                    "invalid-manifest",
                )),
            }
        }
    }

    Ok(())
}

fn validate_security_patterns(files: &[ProjectFile], errors: &mut Vec<ValidationError>) -> Result<()> {
    for file in files {
        if !is_script_file(&file.relative_path) {
            continue;
        }

        let content = fs::read_to_string(&file.absolute_path)
            .with_context(|| format!("failed to read {}", file.absolute_path.display()))?;
        for (line_index, line) in content.lines().enumerate() {
            let line_number = line_index + 1;
            for (needle, message, severity) in SECURITY_RULES {
                if !line.contains(needle) {
                    continue;
                }

                let validation_error = match severity {
                    ValidationSeverity::Error => ValidationError::error(
                        Some(file.relative_path.as_str()),
                        Some(line_number),
                        *message,
                        "forbidden-sink",
                    ),
                    ValidationSeverity::Warning => ValidationError::warning(
                        Some(file.relative_path.as_str()),
                        Some(line_number),
                        *message,
                        "forbidden-sink",
                    ),
                };
                errors.push(validation_error);
            }
        }
    }
    Ok(())
}

fn is_script_file(path: &str) -> bool {
    has_allowed_extension(path, &["ts", "tsx", "js", "jsx"])
}
