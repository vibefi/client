(() => {
  const PROVIDER_ID = "vibefi-provider";
  const callbacks = new Map<number, { resolve: (value: unknown) => void; reject: (error: unknown) => void }>();
  let nextId = 1;

  const listeners = new Map<string, Set<(...args: unknown[]) => void>>();
  function on(event: string, handler: (...args: unknown[]) => void) {
    if (typeof handler !== "function") return;
    const set = listeners.get(event) ?? new Set<(...args: unknown[]) => void>();
    set.add(handler);
    listeners.set(event, set);
  }
  function off(event: string, handler: (...args: unknown[]) => void) {
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
        // Ignore listener exceptions to mirror wallet provider behavior.
      }
    }
  }

  function handleResponse(id: number, result: unknown, error: unknown) {
    const callback = callbacks.get(id);
    if (!callback) return;
    callbacks.delete(id);
    if (error) callback.reject(error);
    else callback.resolve(result);
  }

  async function request(args: { method: string; params?: unknown[] }) {
    const method = args?.method;
    const params = Array.isArray(args?.params) ? args.params : [];
    return await new Promise((resolve, reject) => {
      const id = nextId++;
      callbacks.set(id, { resolve, reject });
      window.ipc.postMessage(
        JSON.stringify({
          id,
          providerId: PROVIDER_ID,
          method,
          params,
        })
      );
    });
  }

  (window as any).__WryEthereumEmit = (event: string, payload: unknown) => {
    emit(event, payload);
  };

  (window as any).__WryEthereumResolve = (id: number, result: unknown, error: unknown) => {
    handleResponse(id, result ?? null, error ?? null);
  };

  (window as any).__VibefiHostDispatch = (message: any) => {
    if (!message || typeof message !== "object") return;
    const kind = message.kind;
    const payload = message.payload;
    switch (kind) {
      case "rpcResponse":
        if (payload) {
          handleResponse(payload.id, payload.result ?? null, payload.error ?? null);
        }
        break;
      case "providerEvent":
        if (payload) {
          emit(payload.event, payload.value);
        }
        break;
      case "walletconnectPairing":
        window.dispatchEvent(
          new CustomEvent("vibefi:walletconnect-pairing", { detail: payload ?? {} })
        );
        break;
      case "tabbarUpdate":
        if (typeof (window as any).updateTabs === "function" && payload) {
          (window as any).updateTabs(payload.tabs ?? [], payload.activeIndex ?? 0);
        }
        break;
      default:
        break;
    }
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

  if (!(window as any).ethereum) {
    Object.defineProperty(window, "ethereum", {
      value: ethereum,
      configurable: false,
      enumerable: true,
      writable: false,
    });
  }

  (window as any).vibefi = {
    request: ({ method, params }: { method: string; params?: unknown[] }) =>
      new Promise((resolve, reject) => {
        const id = nextId++;
        callbacks.set(id, { resolve, reject });
        window.ipc.postMessage(
          JSON.stringify({
            id,
            providerId: "vibefi-launcher",
            method,
            params: Array.isArray(params) ? params : [],
          })
        );
      }),
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
