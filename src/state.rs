use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::devnet::DevnetContext;
use crate::walletconnect::WalletConnectBridge;

#[derive(Debug, Clone, Copy)]
pub struct Chain {
    pub chain_id: u64,
}

impl Default for Chain {
    fn default() -> Self {
        // Ethereum mainnet
        Self { chain_id: 1 }
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpcRequest {
    pub id: u64,
    #[serde(default)]
    pub provider_id: Option<String>,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Clone)]
pub enum UserEvent {
    Ipc(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalletBackend {
    Local,
    WalletConnect,
}

#[derive(Debug, Serialize)]
pub struct ProviderInfo {
    pub name: String,
    pub chain_id: String,
    pub backend: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub walletconnect_uri: Option<String>,
}

#[derive(Debug, Default)]
pub struct WalletState {
    pub authorized: bool,
    pub chain: Chain,
    pub account: Option<String>,
    pub walletconnect_uri: Option<String>,
}

#[derive(Clone)]
pub struct AppState {
    pub wallet: Arc<Mutex<WalletState>>,
    pub wallet_backend: WalletBackend,
    pub signer: Option<Arc<PrivateKeySigner>>,
    pub walletconnect: Option<Arc<Mutex<WalletConnectBridge>>>,
    pub devnet: Option<DevnetContext>,
    pub current_bundle: Arc<Mutex<Option<PathBuf>>>,
}

impl AppState {
    pub fn local_signer(&self) -> Option<Arc<PrivateKeySigner>> {
        self.signer.as_ref().cloned()
    }

    pub fn local_signer_address(&self) -> Option<String> {
        self.signer
            .as_ref()
            .map(|signer| format!("0x{:x}", signer.address()))
    }

    pub fn account(&self) -> Option<String> {
        let ws = self.wallet.lock().unwrap();
        if let Some(account) = ws.account.clone() {
            return Some(account);
        }
        drop(ws);
        self.local_signer_address()
    }

    pub fn chain_id_hex(&self) -> String {
        let chain_id = self.wallet.lock().unwrap().chain.chain_id;
        format!("0x{:x}", chain_id)
    }
}

pub struct LauncherConfig {
    pub devnet_path: Option<PathBuf>,
    pub rpc_url: String,
    pub ipfs_api: String,
    pub ipfs_gateway: String,
    pub cache_dir: PathBuf,
    pub wallet_backend: WalletBackend,
    pub wc_project_id: Option<String>,
    pub wc_relay_url: Option<String>,
}
