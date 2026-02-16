import { IpcClient } from "./ipc/client";
import { PROVIDER_IDS } from "./ipc/contracts";
import { handleHostDispatch } from "./ipc/host-dispatch";

type Eip1193RequestArgs = {
  method: string;
  params?: unknown[];
};

type Listener = (...args: unknown[]) => void;
type IpfsListener = (payload: unknown) => void;

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
    vibefiIpfs?: {
      request: (args: Eip1193RequestArgs) => Promise<unknown>;
      requestWithId: (
        args: Eip1193RequestArgs
      ) => { ipcId: number; response: Promise<unknown> };
      on: (event: "progress", handler: IpfsListener) => void;
      off: (event: "progress", handler: IpfsListener) => void;
      removeListener: (event: "progress", handler: IpfsListener) => void;
    };
    updateTabs?: (tabs: unknown[], activeIndex: number) => void;
  };

  const ipc = new IpcClient();
  const listeners = new Map<string, Set<Listener>>();
  const ipfsListeners = new Map<string, Set<IpfsListener>>();

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
      } catch (error) {
        // Keep provider semantics: listener failures should not break dispatch.
        console.warn("[vibefi:preload] listener threw during emit", event, error);
      }
    }
  }

  function onIpfs(event: string, handler: IpfsListener) {
    if (typeof handler !== "function") return;
    const set = ipfsListeners.get(event) ?? new Set<IpfsListener>();
    set.add(handler);
    ipfsListeners.set(event, set);
  }

  function offIpfs(event: string, handler: IpfsListener) {
    const set = ipfsListeners.get(event);
    if (!set) return;
    set.delete(handler);
  }

  function emitIpfs(event: string, payload: unknown) {
    const set = ipfsListeners.get(event);
    if (!set) return;
    for (const handler of Array.from(set)) {
      try {
        handler(payload);
      } catch (error) {
        console.warn("[vibefi:preload] ipfs listener threw during emit", event, error);
      }
    }
  }

  async function request(args: Eip1193RequestArgs) {
    const method = args?.method;
    const params = Array.isArray(args?.params) ? args.params : [];
    return await ipc.request(PROVIDER_IDS.provider, method, params);
  }

  function requestIpfs(args: Eip1193RequestArgs): Promise<unknown> {
    const method = args?.method;
    const params = Array.isArray(args?.params) ? args.params : [];
    return ipc.request(PROVIDER_IDS.ipfs, method, params);
  }

  function requestIpfsWithId(
    args: Eip1193RequestArgs
  ): { ipcId: number; response: Promise<unknown> } {
    const method = args?.method;
    const params = Array.isArray(args?.params) ? args.params : [];
    const { id, promise } = ipc.requestWithId(PROVIDER_IDS.ipfs, method, params);
    return { ipcId: id, response: promise };
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
        if (payload.event === "vibefiIpfsProgress") {
          emitIpfs("progress", payload.value);
          return;
        }
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

  globalWindow.vibefiIpfs = {
    request: requestIpfs,
    requestWithId: requestIpfsWithId,
    on: onIpfs,
    off: offIpfs,
    removeListener: offIpfs,
  };

  Promise.resolve().then(async () => {
    try {
      const chainId = await request({ method: "eth_chainId", params: [] });
      emit("connect", { chainId });
    } catch (error) {
      // Ignore connect bootstrap failure.
      console.debug("[vibefi:preload] connect bootstrap failed", error);
    }
  });
})();
