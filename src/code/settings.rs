use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const SETTINGS_FILE: &str = "code-settings.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeApiKeys {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openai: Option<String>,
}

impl CodeApiKeys {
    pub fn normalize(value: Option<String>) -> Option<String> {
        value
            .and_then(|value| {
                let trimmed = value.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeLlmConfig {
    pub provider: String,
    pub model: String,
}

impl Default for CodeLlmConfig {
    fn default() -> Self {
        Self {
            provider: "claude".to_string(),
            model: "claude-sonnet-4-6".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeSettings {
    #[serde(default)]
    pub api_keys: CodeApiKeys,
    #[serde(default)]
    pub llm_config: CodeLlmConfig,
}

impl Default for CodeSettings {
    fn default() -> Self {
        Self {
            api_keys: CodeApiKeys::default(),
            llm_config: CodeLlmConfig::default(),
        }
    }
}

fn settings_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(SETTINGS_FILE)
}

pub fn load_settings(workspace_root: &Path) -> CodeSettings {
    let path = settings_path(workspace_root);
    if !path.exists() {
        return CodeSettings::default();
    }
    match fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str(&raw) {
            Ok(settings) => settings,
            Err(err) => {
                tracing::warn!(path = %path.display(), error = %err, "failed to parse code settings; using defaults");
                CodeSettings::default()
            }
        },
        Err(err) => {
            tracing::warn!(path = %path.display(), error = %err, "failed to read code settings; using defaults");
            CodeSettings::default()
        }
    }
}

pub fn save_settings(workspace_root: &Path, settings: &CodeSettings) -> Result<()> {
    fs::create_dir_all(workspace_root).with_context(|| {
        format!("failed to create code workspace root {}", workspace_root.display())
    })?;
    let path = settings_path(workspace_root);
    let json = serde_json::to_string_pretty(settings).context("serialize code settings")?;
    fs::write(&path, json).context("write code settings")?;
    Ok(())
}
