use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::IpfsFetchBackend;
use crate::rpc_manager::RpcEndpoint;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpfsUserSettings {
    #[serde(default)]
    pub fetch_backend: Option<IpfsFetchBackend>,
    #[serde(default)]
    pub gateway_endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSettings {
    #[serde(default)]
    pub rpc_endpoints: Vec<RpcEndpoint>,
    #[serde(default)]
    pub max_concurrent_rpc: Option<usize>,
    #[serde(default)]
    pub ipfs: IpfsUserSettings,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            rpc_endpoints: Vec::new(),
            max_concurrent_rpc: None,
            ipfs: IpfsUserSettings::default(),
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
        Ok(raw) => match serde_json::from_str(&raw) {
            Ok(settings) => settings,
            Err(err) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %err,
                    "failed to parse settings.json; using defaults"
                );
                UserSettings::default()
            }
        },
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                error = %err,
                "failed to read settings.json; using defaults"
            );
            UserSettings::default()
        }
    }
}

pub fn save_settings(config_path: &Path, settings: &UserSettings) -> Result<()> {
    let path = settings_path_from_config(config_path);
    let json = serde_json::to_string_pretty(settings).context("serialize settings")?;
    fs::write(&path, json).context("write settings.json")?;
    Ok(())
}
