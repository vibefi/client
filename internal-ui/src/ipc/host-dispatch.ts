import type {
  HostDispatchMessage,
  ProviderEventPayload,
  RpcResponsePayload,
  TabbarUpdatePayload,
  WalletconnectPairingPayload,
} from "./contracts";

export type HostDispatchHandlers = {
  onRpcResponse?: (payload: RpcResponsePayload) => void;
  onProviderEvent?: (payload: ProviderEventPayload) => void;
  onWalletconnectPairing?: (payload: WalletconnectPairingPayload) => void;
  onTabbarUpdate?: (payload: TabbarUpdatePayload) => void;
};

export function handleHostDispatch(message: unknown, handlers: HostDispatchHandlers) {
  if (!message || typeof message !== "object") return;

  const candidate = message as Partial<HostDispatchMessage> & {
    kind?: unknown;
    payload?: unknown;
  };

  if (candidate.kind === "rpcResponse") {
    handlers.onRpcResponse?.((candidate.payload ?? {}) as RpcResponsePayload);
    return;
  }
  if (candidate.kind === "providerEvent") {
    handlers.onProviderEvent?.((candidate.payload ?? {}) as ProviderEventPayload);
    return;
  }
  if (candidate.kind === "walletconnectPairing") {
    handlers.onWalletconnectPairing?.((candidate.payload ?? {}) as WalletconnectPairingPayload);
    return;
  }
  if (candidate.kind === "tabbarUpdate") {
    handlers.onTabbarUpdate?.((candidate.payload ?? {}) as TabbarUpdatePayload);
  }
}
