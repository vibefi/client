import { handleHostDispatch } from "./ipc/host-dispatch";

declare global {
  interface Window {
    __WryEthereumResolve?: (id: number, result: unknown, error: unknown) => void;
    __WryEthereumEmit?: (event: string, payload: unknown) => void;
    __VibefiHostDispatch?: (message: unknown) => void;
  }
}

(() => {
  window.__WryEthereumResolve =
    window.__WryEthereumResolve ||
    function () {
      // Set by wallet selector app.
    };

  window.__WryEthereumEmit =
    window.__WryEthereumEmit ||
    function () {
      // Not used in selector.
    };

  window.__VibefiHostDispatch =
    window.__VibefiHostDispatch ||
    function (message: unknown) {
      handleHostDispatch(message, {
        onRpcResponse: (payload) => {
          window.__WryEthereumResolve?.(payload.id, payload.result ?? null, payload.error ?? null);
        },
        onWalletconnectPairing: (payload) => {
          window.dispatchEvent(
            new CustomEvent("vibefi:walletconnect-pairing", { detail: payload ?? {} })
          );
        },
      });
    };
})();
