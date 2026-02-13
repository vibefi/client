use anyhow::{Context, Result};
use reqwest::blocking::Client as HttpClient;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IpfsFetchBackend {
    #[serde(rename = "localnode")]
    LocalNode,
    #[serde(rename = "helia")]
    Helia,
}

impl Default for IpfsFetchBackend {
    fn default() -> Self {
        Self::Helia
    }
}

impl IpfsFetchBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LocalNode => "localnode",
            Self::Helia => "helia",
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[allow(non_snake_case)]
pub struct AppConfig {
    pub chainId: u64,

    #[serde(default)]
    pub deployBlock: Option<u64>,

    #[serde(default)]
    pub dappRegistry: String,

    #[serde(default)]
    pub developerPrivateKey: Option<String>,

    #[serde(default = "default_rpc_url")]
    pub rpcUrl: String,

    #[serde(default)]
    pub localNetwork: bool,

    #[serde(default)]
    pub ipfsApi: Option<String>,

    #[serde(default)]
    pub ipfsGateway: Option<String>,

    #[serde(default)]
    pub ipfsFetchBackend: IpfsFetchBackend,

    #[serde(default = "default_ipfs_helia_gateways")]
    pub ipfsHeliaGateways: Vec<String>,

    #[serde(default = "default_ipfs_helia_routers")]
    pub ipfsHeliaRouters: Vec<String>,

    #[serde(default = "default_ipfs_helia_timeout_ms")]
    pub ipfsHeliaTimeoutMs: u64,

    #[serde(default)]
    pub cacheDir: Option<String>,

    #[serde(default)]
    pub walletConnect: Option<WalletConnectConfig>,
}

fn default_rpc_url() -> String {
    "http://127.0.0.1:8546".to_string()
}

fn default_ipfs_helia_gateways() -> Vec<String> {
    vec![
        "https://trustless-gateway.link".to_string(),
        "https://cloudflare-ipfs.com".to_string(),
        "https://ipfs.filebase.io".to_string(),
        "https://ipfs.io".to_string(),
        "https://dweb.link".to_string(),
    ]
}

fn default_ipfs_helia_routers() -> Vec<String> {
    vec![
        "https://delegated-ipfs.dev".to_string(),
        "https://cid.contact".to_string(),
        "https://indexer.pinata.cloud".to_string(),
    ]
}

fn default_ipfs_helia_timeout_ms() -> u64 {
    30_000
}

#[derive(Debug, Deserialize, Clone)]
#[allow(non_snake_case)]
pub struct WalletConnectConfig {
    #[serde(default)]
    pub projectId: Option<String>,
    #[serde(default)]
    pub relayUrl: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NetworkContext {
    pub config: AppConfig,
    pub rpc_url: String,
    pub ipfs_api: String,
    pub ipfs_gateway: String,
    pub ipfs_fetch_backend: IpfsFetchBackend,
    pub ipfs_helia_gateways: Vec<String>,
    pub ipfs_helia_routers: Vec<String>,
    pub ipfs_helia_timeout_ms: u64,
    pub cache_dir: PathBuf,
    pub http: HttpClient,
}

pub fn load_config(path: &Path) -> Result<AppConfig> {
    let raw = fs::read_to_string(path).context("read config file")?;
    let cfg: AppConfig = serde_json::from_str(&raw).context("parse config file")?;
    Ok(cfg)
}

pub fn build_network_context(config: AppConfig) -> NetworkContext {
    let rpc_url = config.rpcUrl.clone();
    let ipfs_api = config
        .ipfsApi
        .clone()
        .unwrap_or_else(|| "http://127.0.0.1:5001".to_string());
    let ipfs_gateway = config
        .ipfsGateway
        .clone()
        .unwrap_or_else(|| "http://127.0.0.1:8080".to_string());
    let ipfs_fetch_backend = config.ipfsFetchBackend;
    let ipfs_helia_gateways = config.ipfsHeliaGateways.clone();
    let ipfs_helia_routers = config.ipfsHeliaRouters.clone();
    let ipfs_helia_timeout_ms = config.ipfsHeliaTimeoutMs;
    let cache_dir = config
        .cacheDir
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::cache_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("VibeFi")
        });
    NetworkContext {
        config,
        rpc_url,
        ipfs_api,
        ipfs_gateway,
        ipfs_fetch_backend,
        ipfs_helia_gateways,
        ipfs_helia_routers,
        ipfs_helia_timeout_ms,
        cache_dir,
        http: HttpClient::new(),
    }
}
