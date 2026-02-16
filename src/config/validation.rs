use anyhow::{Result, bail};

use super::app_config::AppConfig;

/// Validate an `AppConfig` after deserialization.
///
/// Returns an error if:
/// - `chainId` is 0
/// - `dappRegistry` is non-empty but not valid hex (with optional 0x prefix)
/// - `rpcUrl` is not a valid URL scheme (http/https/ws/wss)
pub fn validate_app_config(config: &AppConfig) -> Result<()> {
    if config.chainId == 0 {
        bail!("chainId must not be 0");
    }

    if !config.dappRegistry.is_empty() {
        let hex_str = config
            .dappRegistry
            .strip_prefix("0x")
            .unwrap_or(&config.dappRegistry);
        if hex_str.is_empty() || hex::decode(hex_str).is_err() {
            bail!("dappRegistry is not valid hex: {:?}", config.dappRegistry);
        }
    }

    if !config.rpcUrl.is_empty() {
        let lower = config.rpcUrl.to_ascii_lowercase();
        if !lower.starts_with("http://")
            && !lower.starts_with("https://")
            && !lower.starts_with("ws://")
            && !lower.starts_with("wss://")
        {
            bail!(
                "rpcUrl must start with http://, https://, ws://, or wss://: {:?}",
                config.rpcUrl
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::app_config::IpfsFetchBackend;

    fn minimal_config() -> AppConfig {
        AppConfig {
            chainId: 1,
            deployBlock: None,
            dappRegistry: String::new(),
            developerPrivateKey: None,
            rpcUrl: "http://127.0.0.1:8546".to_string(),
            localNetwork: false,
            ipfsApi: None,
            ipfsGateway: None,
            ipfsFetchBackend: IpfsFetchBackend::default(),
            ipfsHeliaGateways: Vec::new(),
            ipfsHeliaRouters: Vec::new(),
            ipfsHeliaTimeoutMs: 30_000,
            cacheDir: None,
            walletConnect: None,
        }
    }

    #[test]
    fn valid_minimal_config() {
        assert!(validate_app_config(&minimal_config()).is_ok());
    }

    #[test]
    fn chain_id_zero_rejected() {
        let mut cfg = minimal_config();
        cfg.chainId = 0;
        assert!(validate_app_config(&cfg).is_err());
    }

    #[test]
    fn invalid_dapp_registry_rejected() {
        let mut cfg = minimal_config();
        cfg.dappRegistry = "not-hex".to_string();
        assert!(validate_app_config(&cfg).is_err());
    }

    #[test]
    fn valid_dapp_registry_accepted() {
        let mut cfg = minimal_config();
        cfg.dappRegistry = "0xaabbccdd".to_string();
        assert!(validate_app_config(&cfg).is_ok());
    }

    #[test]
    fn invalid_rpc_url_rejected() {
        let mut cfg = minimal_config();
        cfg.rpcUrl = "ftp://bad-scheme".to_string();
        assert!(validate_app_config(&cfg).is_err());
    }

    #[test]
    fn websocket_rpc_url_accepted() {
        let mut cfg = minimal_config();
        cfg.rpcUrl = "wss://mainnet.infura.io".to_string();
        assert!(validate_app_config(&cfg).is_ok());
    }
}
