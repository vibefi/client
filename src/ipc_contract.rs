use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROVIDER_ID_WALLET: &str = "vibefi-wallet";
pub const PROVIDER_ID_LAUNCHER: &str = "vibefi-launcher";
pub const PROVIDER_ID_TABBAR: &str = "vibefi-tabbar";
pub const PROVIDER_ID_PROVIDER: &str = "vibefi-provider";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnownProviderId {
    Provider,
    Wallet,
    Launcher,
    Tabbar,
}

impl KnownProviderId {
    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            PROVIDER_ID_PROVIDER => Some(Self::Provider),
            PROVIDER_ID_WALLET => Some(Self::Wallet),
            PROVIDER_ID_LAUNCHER => Some(Self::Launcher),
            PROVIDER_ID_TABBAR => Some(Self::Tabbar),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpcRequest {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub provider_id: Option<String>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl IpcRequest {
    pub fn provider(&self) -> Option<KnownProviderId> {
        self.provider_id
            .as_deref()
            .and_then(KnownProviderId::from_str)
    }

    pub fn wallet_selector_method(&self) -> Option<WalletSelectorMethod> {
        WalletSelectorMethod::from_str(self.method.as_str())
    }

    pub fn tabbar_method(&self) -> Option<TabbarMethod> {
        TabbarMethod::from_str(self.method.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalletSelectorMethod {
    ConnectLocal,
    ConnectWalletConnect,
    ConnectHardware,
}

impl WalletSelectorMethod {
    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "vibefi_connectLocal" => Some(Self::ConnectLocal),
            "vibefi_connectWalletConnect" => Some(Self::ConnectWalletConnect),
            "vibefi_connectHardware" => Some(Self::ConnectHardware),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabbarMethod {
    SwitchTab,
    CloseTab,
}

impl TabbarMethod {
    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "switchTab" => Some(Self::SwitchTab),
            "closeTab" => Some(Self::CloseTab),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HostDispatchKind {
    RpcResponse,
    ProviderEvent,
    WalletconnectPairing,
    TabbarUpdate,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostDispatchEnvelope<T: Serialize> {
    pub kind: HostDispatchKind,
    pub payload: T,
}

#[derive(Debug, Clone, Serialize)]
pub struct RpcResponseError {
    pub code: i64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RpcResponsePayload {
    pub id: u64,
    pub result: Value,
    pub error: Option<RpcResponseError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderEventPayload {
    pub event: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletconnectPairingPayload {
    pub uri: String,
    pub qr_svg: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabbarUpdatePayload {
    pub tabs: Vec<Value>,
    pub active_index: usize,
}
