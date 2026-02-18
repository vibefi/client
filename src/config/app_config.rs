use serde::{Deserialize, Serialize};

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
    pub studioDappId: Option<u64>,

    #[serde(default)]
    pub developerPrivateKey: Option<String>,

    #[serde(default = "default_rpc_url")]
    pub rpcUrl: String,

    #[serde(default)]
    pub testNetwork: bool,

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

pub(crate) fn default_ipfs_helia_gateways() -> Vec<String> {
    vec![
        "https://trustless-gateway.link".to_string(),
        "https://cloudflare-ipfs.com".to_string(),
        "https://ipfs.filebase.io".to_string(),
        "https://ipfs.io".to_string(),
        "https://dweb.link".to_string(),
    ]
}

pub(crate) fn default_ipfs_helia_routers() -> Vec<String> {
    vec![
        "https://delegated-ipfs.dev".to_string(),
        "https://cid.contact".to_string(),
        "https://indexer.pinata.cloud".to_string(),
    ]
}

fn default_ipfs_helia_timeout_ms() -> u64 {
    15_000
}

#[derive(Debug, Deserialize, Clone)]
#[allow(non_snake_case)]
pub struct WalletConnectConfig {
    #[serde(default)]
    pub projectId: Option<String>,
    #[serde(default)]
    pub relayUrl: Option<String>,
}
