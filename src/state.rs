use alloy_signer_local::PrivateKeySigner;
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use tao::event_loop::EventLoopProxy;

use crate::config::NetworkContext;
use crate::hardware::HardwareDevice;
use crate::rpc_manager::RpcEndpointManager;
use crate::walletconnect::{WalletConnectBridge, WalletConnectSession};

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

#[derive(Debug, Clone)]
pub enum UserEvent {
    Ipc {
        webview_id: String,
        msg: String,
    },
    OpenWalletSelector,
    OpenSettings,
    WalletConnectPairing {
        uri: String,
        qr_svg: String,
    },
    WalletConnectResult {
        webview_id: String,
        ipc_id: u64,
        result: Result<WalletConnectSession, String>,
    },
    HardwareSignResult {
        webview_id: String,
        ipc_id: u64,
        result: Result<String, String>,
    },
    RpcResult {
        webview_id: String,
        ipc_id: u64,
        result: Result<serde_json::Value, String>,
    },
    ProviderEvent {
        webview_id: String,
        event: String,
        value: serde_json::Value,
    },
    CloseWalletSelector,
    TabAction(TabAction),
}

#[derive(Debug, Clone)]
pub enum TabAction {
    OpenApp { name: String, dist_dir: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalletBackend {
    Local,
    WalletConnect,
    Hardware,
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

/// Tracks a pending `eth_requestAccounts` that is waiting for the user to
/// pick a wallet backend in the selector tab.
#[derive(Debug, Clone)]
pub struct PendingConnect {
    pub webview_id: String,
    pub ipc_id: u64,
}

#[derive(Clone)]
pub struct AppState {
    pub wallet: Arc<Mutex<WalletState>>,
    pub wallet_backend: Arc<Mutex<Option<WalletBackend>>>,
    pub signer: Arc<Mutex<Option<Arc<PrivateKeySigner>>>>,
    pub walletconnect: Arc<Mutex<Option<Arc<Mutex<WalletConnectBridge>>>>>,
    pub hardware_signer: Arc<Mutex<Option<HardwareDevice>>>,
    pub network: Option<NetworkContext>,
    pub proxy: EventLoopProxy<UserEvent>,
    pub pending_connect: Arc<Mutex<Option<PendingConnect>>>,
    /// Webview ID of the wallet selector tab, if open.
    pub selector_webview_id: Arc<Mutex<Option<String>>>,
    pub rpc_manager: Arc<Mutex<Option<RpcEndpointManager>>>,
    pub config_path: Option<PathBuf>,
    pub settings_webview_id: Arc<Mutex<Option<String>>>,
}

impl AppState {
    pub fn local_signer(&self) -> Option<Arc<PrivateKeySigner>> {
        self.signer.lock().unwrap().as_ref().cloned()
    }

    pub fn local_signer_address(&self) -> Option<String> {
        self.signer
            .lock()
            .unwrap()
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

    pub fn get_wallet_backend(&self) -> Option<WalletBackend> {
        *self.wallet_backend.lock().unwrap()
    }
}
