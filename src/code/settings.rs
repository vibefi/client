use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const SETTINGS_FILE: &str = "code-settings.json";

const fn default_anvil_auto_start_on_open() -> bool {
    true
}

const fn default_anvil_port() -> u16 {
    9545
}

const fn default_anvil_chain_id() -> u64 {
    1
}

fn default_ipfs_pin_endpoint() -> String {
    "http://127.0.0.1:5001".to_string()
}

fn default_protocol_relay_endpoint() -> String {
    "https://ipfs.vibefi.dev".to_string()
}

fn default_four_everland_endpoint() -> String {
    "https://api.4everland.dev".to_string()
}

fn default_pinata_endpoint() -> String {
    "https://api.pinata.cloud".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeAnvilConfig {
    #[serde(default = "default_anvil_auto_start_on_open")]
    pub auto_start_on_open: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork_url: Option<String>,
    #[serde(default = "default_anvil_port")]
    pub port: u16,
    #[serde(default = "default_anvil_chain_id")]
    pub chain_id: u64,
}

impl Default for CodeAnvilConfig {
    fn default() -> Self {
        Self {
            auto_start_on_open: default_anvil_auto_start_on_open(),
            fork_url: None,
            port: default_anvil_port(),
            chain_id: default_anvil_chain_id(),
        }
    }
}

impl CodeAnvilConfig {
    pub fn normalized(mut self) -> Self {
        self.fork_url = self.fork_url.and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        });
        if self.port == 0 {
            self.port = default_anvil_port();
        }
        if self.chain_id == 0 {
            self.chain_id = default_anvil_chain_id();
        }
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UploadProvider {
    ProtocolRelay,
    FourEverland,
    Pinata,
    LocalNode,
}

impl Default for UploadProvider {
    fn default() -> Self {
        Self::ProtocolRelay
    }
}

impl UploadProvider {
    pub fn label(&self) -> &'static str {
        match self {
            Self::ProtocolRelay => "Protocol Relay",
            Self::FourEverland => "4EVERLAND",
            Self::Pinata => "Pinata",
            Self::LocalNode => "Local IPFS Node",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolRelayUploadConfig {
    #[serde(default = "default_protocol_relay_endpoint")]
    pub endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

impl Default for ProtocolRelayUploadConfig {
    fn default() -> Self {
        Self {
            endpoint: default_protocol_relay_endpoint(),
            api_key: None,
        }
    }
}

impl ProtocolRelayUploadConfig {
    pub fn normalized(mut self) -> Self {
        self.endpoint = self.endpoint.trim().to_string();
        self.api_key = self.api_key.and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        });
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FourEverlandUploadConfig {
    #[serde(default = "default_four_everland_endpoint")]
    pub endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
}

impl Default for FourEverlandUploadConfig {
    fn default() -> Self {
        Self {
            endpoint: default_four_everland_endpoint(),
            access_token: None,
        }
    }
}

impl FourEverlandUploadConfig {
    pub fn normalized(mut self) -> Self {
        self.endpoint = self.endpoint.trim().to_string();
        if self.endpoint.is_empty() {
            self.endpoint = default_four_everland_endpoint();
        }
        self.access_token = self.access_token.and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        });
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinataUploadConfig {
    #[serde(default = "default_pinata_endpoint")]
    pub endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

impl Default for PinataUploadConfig {
    fn default() -> Self {
        Self {
            endpoint: default_pinata_endpoint(),
            api_key: None,
        }
    }
}

impl PinataUploadConfig {
    pub fn normalized(mut self) -> Self {
        self.endpoint = self.endpoint.trim().to_string();
        if self.endpoint.is_empty() {
            self.endpoint = default_pinata_endpoint();
        }
        self.api_key = self.api_key.and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        });
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalNodeUploadConfig {
    #[serde(default = "default_ipfs_pin_endpoint")]
    pub endpoint: String,
}

impl Default for LocalNodeUploadConfig {
    fn default() -> Self {
        Self {
            endpoint: default_ipfs_pin_endpoint(),
        }
    }
}

impl LocalNodeUploadConfig {
    pub fn normalized(mut self) -> Self {
        self.endpoint = self.endpoint.trim().to_string();
        if self.endpoint.is_empty() {
            self.endpoint = default_ipfs_pin_endpoint();
        }
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadConfig {
    #[serde(default)]
    pub provider: UploadProvider,
    #[serde(default)]
    pub protocol_relay: ProtocolRelayUploadConfig,
    #[serde(default)]
    pub four_everland: FourEverlandUploadConfig,
    #[serde(default)]
    pub pinata: PinataUploadConfig,
    #[serde(default)]
    pub local_node: LocalNodeUploadConfig,
}

impl Default for UploadConfig {
    fn default() -> Self {
        Self {
            provider: UploadProvider::default(),
            protocol_relay: ProtocolRelayUploadConfig::default(),
            four_everland: FourEverlandUploadConfig::default(),
            pinata: PinataUploadConfig::default(),
            local_node: LocalNodeUploadConfig::default(),
        }
    }
}

impl UploadConfig {
    pub fn normalized(mut self) -> Self {
        self.protocol_relay = self.protocol_relay.normalized();
        self.four_everland = self.four_everland.normalized();
        self.pinata = self.pinata.normalized();
        self.local_node = self.local_node.normalized();
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeSettings {
    #[serde(default)]
    pub anvil: CodeAnvilConfig,
    #[serde(default)]
    pub upload: UploadConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_project_path: Option<String>,
}

impl Default for CodeSettings {
    fn default() -> Self {
        Self {
            anvil: CodeAnvilConfig::default(),
            upload: UploadConfig::default(),
            last_project_path: None,
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
        format!(
            "failed to create code workspace root {}",
            workspace_root.display()
        )
    })?;
    let path = settings_path(workspace_root);
    let json = serde_json::to_string_pretty(settings).context("serialize code settings")?;
    fs::write(&path, json).context("write code settings")?;
    Ok(())
}
