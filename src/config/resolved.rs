use reqwest::blocking::Client as HttpClient;
use std::path::PathBuf;

use super::app_config::IpfsFetchBackend;

/// Single resolved configuration built once at startup.
///
/// Merges CLI arguments, the deployment JSON config (`AppConfig`), user
/// settings, and environment variable overrides into one struct.
///
/// Fields annotated *[deploy]* originate from the deployment JSON.
/// Fields annotated *[client]* are client-specific defaults or env overrides.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    // -- Network (deploy) --
    pub chain_id: u64,
    pub deploy_block: Option<u64>,
    pub dapp_registry: String,
    pub local_network: bool,
    pub rpc_url: String,

    // -- IPFS (deploy + client override) --
    pub ipfs_api: String,
    pub ipfs_gateway: String,
    pub ipfs_fetch_backend: IpfsFetchBackend,
    pub ipfs_helia_gateways: Vec<String>,
    pub ipfs_helia_routers: Vec<String>,
    pub ipfs_helia_timeout_ms: u64,

    // -- WalletConnect (deploy + env override) --
    pub walletconnect_project_id: Option<String>,
    pub walletconnect_relay_url: Option<String>,

    // -- Developer (deploy) --
    pub developer_private_key: Option<String>,

    // -- Paths (client) --
    pub cache_dir: PathBuf,
    pub config_path: Option<PathBuf>,

    // -- UI (client) --
    pub enable_devtools: bool,

    // -- HTTP (client) --
    pub http_client: HttpClient,
}

impl ResolvedConfig {
    /// Log a summary of the resolved configuration at startup.
    pub fn log_startup_summary(&self) {
        tracing::info!(
            chain_id = self.chain_id,
            rpc_url = %self.rpc_url,
            local_network = self.local_network,
            dapp_registry = %self.dapp_registry,
            ipfs_backend = self.ipfs_fetch_backend.as_str(),
            ipfs_gateway = %self.ipfs_gateway,
            cache_dir = %self.cache_dir.display(),
            enable_devtools = self.enable_devtools,
            walletconnect = self.walletconnect_project_id.is_some(),
            "resolved configuration"
        );
    }
}
