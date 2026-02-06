use alloy_primitives::Address;
use alloy_signer_local::PrivateKeySigner;
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use crate::devnet::DevnetContext;

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

#[derive(Debug, Serialize)]
pub struct ProviderInfo {
    pub name: &'static str,
    pub chain_id: String,
}

#[derive(Debug, Default)]
pub struct WalletState {
    pub authorized: bool,
    pub chain: Chain,
}

#[derive(Clone)]
pub struct AppState {
    pub wallet: Arc<Mutex<WalletState>>,
    pub signer: Arc<PrivateKeySigner>,
    pub devnet: Option<DevnetContext>,
    pub current_bundle: Arc<Mutex<Option<PathBuf>>>,
}

impl AppState {
    pub fn address(&self) -> Address {
        self.signer.address()
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
}
