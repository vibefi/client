use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::rpc_manager::RpcEndpoint;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSettings {
    #[serde(default)]
    pub rpc_endpoints: Vec<RpcEndpoint>,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            rpc_endpoints: Vec::new(),
        }
    }
}

pub fn settings_path_from_config(config_path: &Path) -> PathBuf {
    config_path.with_file_name("settings.json")
}

pub fn load_settings(config_path: &Path) -> UserSettings {
    let path = settings_path_from_config(config_path);
    if !path.exists() {
        return UserSettings::default();
    }
    match fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => UserSettings::default(),
    }
}

pub fn save_settings(config_path: &Path, settings: &UserSettings) -> Result<()> {
    let path = settings_path_from_config(config_path);
    let json = serde_json::to_string_pretty(settings).context("serialize settings")?;
    fs::write(&path, json).context("write settings.json")?;
    Ok(())
}
