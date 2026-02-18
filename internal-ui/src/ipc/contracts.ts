export const PROVIDER_IDS = {
  provider: "vibefi-provider",
  wallet: "vibefi-wallet",
  launcher: "vibefi-launcher",
  tabbar: "vibefi-tabbar",
  settings: "vibefi-settings",
  ipfs: "vibefi-ipfs",
} as const;

export type ProviderId = (typeof PROVIDER_IDS)[keyof typeof PROVIDER_IDS];

export type IpcRequestMessage = {
  id: number;
  providerId: ProviderId;
  method: string;
  params: unknown[];
};

export type RpcResponsePayload = {
  id: number;
  result: unknown;
  error: unknown;
};

export type ProviderEventPayload = {
  event: string;
  value: unknown;
};

export type WalletconnectPairingPayload = {
  uri?: string;
  qrSvg?: string;
};

export type Tab = {
  id?: string;
  label?: string;
  closable?: boolean;
  clickable?: boolean;
  loading?: boolean;
};

export type TabbarUpdatePayload = {
  tabs?: Tab[];
  activeIndex?: number;
};

export type RpcStatusPayload = {
  webviewId?: string;
  pendingCount?: number;
};

export type HostDispatchMessage =
  | { kind: "rpcResponse"; payload: RpcResponsePayload }
  | { kind: "providerEvent"; payload: ProviderEventPayload }
  | { kind: "walletconnectPairing"; payload: WalletconnectPairingPayload }
  | { kind: "tabbarUpdate"; payload: TabbarUpdatePayload }
  | { kind: "rpcStatus"; payload: RpcStatusPayload };
