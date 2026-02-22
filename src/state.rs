use alloy_signer_local::PrivateKeySigner;
use anyhow::{Result, anyhow};
use serde::Serialize;
use std::{
    collections::HashMap,
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};

use tao::event_loop::EventLoopProxy;

use crate::config::ResolvedConfig;
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
    RpcPendingChanged {
        webview_id: String,
        count: u32,
    },
    ProviderEvent {
        webview_id: String,
        event: String,
        value: serde_json::Value,
    },
    StudioBundleResolved {
        placeholder_id: String,
        result: Result<PathBuf, String>,
    },
    CloseWalletSelector,
    TabAction(TabAction),
    AutomationCommand {
        id: String,
        cmd_type: String,
        target: Option<String>,
        js: Option<String>,
    },
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

#[derive(Debug, Clone)]
pub struct IpfsCapabilityRule {
    pub cid: Option<String>,
    pub paths: Vec<String>,
    pub as_kinds: Vec<String>,
    pub max_bytes: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct AppRuntimeCapabilities {
    pub ipfs_allow: Vec<IpfsCapabilityRule>,
}

#[derive(Clone)]
pub struct AppState {
    pub wallet: Arc<Mutex<WalletState>>,
    pub wallet_backend: Arc<Mutex<Option<WalletBackend>>>,
    pub signer: Arc<Mutex<Option<Arc<PrivateKeySigner>>>>,
    pub walletconnect: Arc<Mutex<Option<Arc<Mutex<WalletConnectBridge>>>>>,
    pub hardware_signer: Arc<Mutex<Option<HardwareDevice>>>,
    pub resolved: Option<Arc<ResolvedConfig>>,
    pub proxy: EventLoopProxy<UserEvent>,
    pub pending_connect: Arc<Mutex<VecDeque<PendingConnect>>>,
    pub app_capabilities: Arc<Mutex<HashMap<String, AppRuntimeCapabilities>>>,
    /// Webview ID of the wallet selector tab, if open.
    pub selector_webview_id: Arc<Mutex<Option<String>>>,
    pub rpc_manager: Arc<Mutex<Option<RpcEndpointManager>>>,
    pub settings_webview_id: Arc<Mutex<Option<String>>>,
    /// Tracks how many RPC passthrough requests are in-flight per webview.
    pub pending_rpc_counts: Arc<Mutex<HashMap<String, u32>>>,
    /// Whether automation mode is enabled (--automation flag).
    pub automation: bool,
}

impl AppState {
    pub fn local_signer(&self) -> Option<Arc<PrivateKeySigner>> {
        self.signer.lock().expect("signer").as_ref().cloned()
    }

    pub fn local_signer_address(&self) -> Option<String> {
        self.signer
            .lock()
            .expect("signer")
            .as_ref()
            .map(|signer| format!("0x{:x}", signer.address()))
    }

    pub fn account(&self) -> Option<String> {
        let ws = self.wallet.lock().expect("wallet");
        if let Some(account) = ws.account.clone() {
            return Some(account);
        }
        drop(ws);
        self.local_signer_address()
    }

    pub fn chain_id_hex(&self) -> String {
        let chain_id = self.wallet.lock().expect("wallet").chain.chain_id;
        format!("0x{:x}", chain_id)
    }

    pub fn get_wallet_backend(&self) -> Option<WalletBackend> {
        *self.wallet_backend.lock().expect("wallet_backend")
    }

    /// Increment the pending RPC count for a webview; returns the new count.
    pub fn increment_rpc_pending(&self, webview_id: &str) -> u32 {
        let mut map = self.pending_rpc_counts.lock().expect("pending_rpc_counts");
        let count = map.entry(webview_id.to_string()).or_insert(0);
        *count += 1;
        *count
    }

    /// Decrement the pending RPC count for a webview; returns the new count.
    pub fn decrement_rpc_pending(&self, webview_id: &str) -> u32 {
        let mut map = self.pending_rpc_counts.lock().expect("pending_rpc_counts");
        let count = map.entry(webview_id.to_string()).or_insert(0);
        *count = count.saturating_sub(1);
        *count
    }

    pub fn app_capabilities_for(&self, webview_id: &str) -> Option<AppRuntimeCapabilities> {
        self.app_capabilities
            .lock()
            .unwrap()
            .get(webview_id)
            .cloned()
    }
}

pub(crate) fn lock_or_err<'a, T>(mutex: &'a Mutex<T>, name: &str) -> Result<MutexGuard<'a, T>> {
    mutex.lock().map_err(|_| anyhow!("poisoned lock: {}", name))
}
