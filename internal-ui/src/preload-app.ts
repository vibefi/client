import { IpcClient } from "./ipc/client";
import { PROVIDER_IDS } from "./ipc/contracts";
import { handleHostDispatch } from "./ipc/host-dispatch";

type Eip1193RequestArgs = {
  method: string;
  params?: unknown[];
};

type Listener = (...args: unknown[]) => void;

declare global {
  interface Window {
    __WryEthereumEmit?: (event: string, payload: unknown) => void;
    __WryEthereumResolve?: (id: number, result: unknown, error: unknown) => void;
    __VibefiHostDispatch?: (message: unknown) => void;
  }
}

(() => {
  const globalWindow = window as Window & {
    ethereum?: {
      isWry: boolean;
      isMetaMask: boolean;
      request: (args: Eip1193RequestArgs) => Promise<unknown>;
      on: (event: string, handler: Listener) => void;
      removeListener: (event: string, handler: Listener) => void;
      off: (event: string, handler: Listener) => void;
      enable: () => Promise<unknown>;
    };
    vibefi?: {
      request: (args: Eip1193RequestArgs) => Promise<unknown>;
    };
    updateTabs?: (tabs: unknown[], activeIndex: number) => void;
  };

  const ipc = new IpcClient();
  const listeners = new Map<string, Set<Listener>>();

  function on(event: string, handler: Listener) {
    if (typeof handler !== "function") return;
    const set = listeners.get(event) ?? new Set<Listener>();
    set.add(handler);
    listeners.set(event, set);
  }

  function off(event: string, handler: Listener) {
    const set = listeners.get(event);
    if (!set) return;
    set.delete(handler);
  }

  function emit(event: string, ...args: unknown[]) {
    const set = listeners.get(event);
    if (!set) return;
    for (const handler of Array.from(set)) {
      try {
        handler(...args);
      } catch {
        // Keep provider semantics: listener failures should not break dispatch.
      }
    }
  }

  async function request(args: Eip1193RequestArgs) {
    const method = args?.method;
    const params = Array.isArray(args?.params) ? args.params : [];
    return await ipc.request(PROVIDER_IDS.provider, method, params);
  }

  globalWindow.__WryEthereumEmit = (event: string, payload: unknown) => {
    emit(event, payload);
  };

  globalWindow.__WryEthereumResolve = (id: number, result: unknown, error: unknown) => {
    ipc.resolve(id, result ?? null, error ?? null);
  };

  globalWindow.__VibefiHostDispatch = (message: unknown) => {
    handleHostDispatch(message, {
      onRpcResponse: (payload) => {
        ipc.resolve(payload.id, payload.result ?? null, payload.error ?? null);
      },
      onProviderEvent: (payload) => {
        emit(payload.event, payload.value);
      },
      onWalletconnectPairing: (payload) => {
        window.dispatchEvent(
          new CustomEvent("vibefi:walletconnect-pairing", { detail: payload ?? {} })
        );
      },
      onTabbarUpdate: (payload) => {
        if (typeof globalWindow.updateTabs === "function") {
          globalWindow.updateTabs(payload.tabs ?? [], payload.activeIndex ?? 0);
        }
      },
    });
  };

  const ethereum = {
    isWry: true,
    isMetaMask: false,
    request,
    on,
    removeListener: off,
    off,
    enable: () => request({ method: "eth_requestAccounts", params: [] }),
  };

  if (!globalWindow.ethereum) {
    Object.defineProperty(globalWindow, "ethereum", {
      value: ethereum,
      configurable: false,
      enumerable: true,
      writable: false,
    });
  }

  globalWindow.vibefi = {
    request: ({ method, params }: Eip1193RequestArgs) => {
      const list = Array.isArray(params) ? params : [];
      return ipc.request(PROVIDER_IDS.launcher, method, list);
    },
  };

  Promise.resolve().then(async () => {
    try {
      const chainId = await request({ method: "eth_chainId", params: [] });
      emit("connect", { chainId });
    } catch {
      // Ignore connect bootstrap failure.
    }
  });
})();
