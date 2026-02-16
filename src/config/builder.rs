use reqwest::blocking::Client as HttpClient;
use std::path::PathBuf;

use super::app_config::{AppConfig, default_ipfs_helia_gateways, default_ipfs_helia_routers};
use super::env::{parse_bool_env, parse_string_env};
use super::resolved::ResolvedConfig;

fn embedded_walletconnect_project_id() -> Option<String> {
    option_env!("VIBEFI_EMBEDDED_WC_PROJECT_ID")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Builds a `ResolvedConfig` by layering:
/// CLI args → AppConfig (deployment JSON) → env var overrides → defaults.
pub struct ConfigBuilder {
    config: AppConfig,
    config_path: Option<PathBuf>,
}

impl ConfigBuilder {
    pub fn new(config: AppConfig, config_path: Option<PathBuf>) -> Self {
        Self {
            config,
            config_path,
        }
    }

    pub fn build(self) -> ResolvedConfig {
        let config = self.config;

        // -- RPC URL: env override takes precedence --
        let rpc_url = parse_string_env("VIBEFI_RPC_URL").unwrap_or_else(|| config.rpcUrl.clone());

        // -- IPFS --
        let ipfs_gateway = config
            .ipfsGateway
            .clone()
            .unwrap_or_else(|| "http://127.0.0.1:8080".to_string());
        let ipfs_fetch_backend = config.ipfsFetchBackend;
        let ipfs_helia_gateways = if config.ipfsHeliaGateways.is_empty() {
            default_ipfs_helia_gateways()
        } else {
            config.ipfsHeliaGateways.clone()
        };
        let ipfs_helia_routers = if config.ipfsHeliaRouters.is_empty() {
            default_ipfs_helia_routers()
        } else {
            config.ipfsHeliaRouters.clone()
        };
        let ipfs_helia_timeout_ms = config.ipfsHeliaTimeoutMs;

        // -- WalletConnect: config → runtime env → compile-time embedded fallback --
        let walletconnect_project_id = config
            .walletConnect
            .as_ref()
            .and_then(|wc| wc.projectId.clone())
            .or_else(|| parse_string_env("VIBEFI_WC_PROJECT_ID"))
            .or_else(embedded_walletconnect_project_id);
        let walletconnect_relay_url = config
            .walletConnect
            .as_ref()
            .and_then(|wc| wc.relayUrl.clone())
            .or_else(|| parse_string_env("VIBEFI_WC_RELAY_URL"));

        // -- Cache dir --
        let cache_dir = config
            .cacheDir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::cache_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("VibeFi")
            });

        // -- Devtools: env override or debug_assertions --
        let enable_devtools = if cfg!(debug_assertions) {
            true
        } else {
            parse_bool_env("VIBEFI_ENABLE_DEVTOOLS").unwrap_or(false)
        };

        ResolvedConfig {
            chain_id: config.chainId,
            deploy_block: config.deployBlock,
            dapp_registry: config.dappRegistry.clone(),
            local_network: config.localNetwork,
            rpc_url,
            ipfs_gateway,
            ipfs_fetch_backend,
            ipfs_helia_gateways,
            ipfs_helia_routers,
            ipfs_helia_timeout_ms,
            walletconnect_project_id,
            walletconnect_relay_url,
            developer_private_key: config.developerPrivateKey.clone(),
            cache_dir,
            config_path: self.config_path,
            enable_devtools,
            http_client: HttpClient::new(),
        }
    }
}
