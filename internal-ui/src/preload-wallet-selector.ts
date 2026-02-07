(() => {
  (window as any).__WryEthereumResolve =
    (window as any).__WryEthereumResolve ||
    function () {
      // set by selector app
    };
  (window as any).__WryEthereumEmit =
    (window as any).__WryEthereumEmit ||
    function () {
      // not used in selector
    };

  (window as any).__VibefiHostDispatch =
    (window as any).__VibefiHostDispatch ||
    function (message: any) {
      if (!message || typeof message !== "object") return;
      const kind = message.kind;
      const payload = message.payload;
      if (kind === "rpcResponse" && payload) {
        (window as any).__WryEthereumResolve(
          payload.id,
          payload.result ?? null,
          payload.error ?? null
        );
        return;
      }
      if (kind === "walletconnectPairing") {
        window.dispatchEvent(
          new CustomEvent("vibefi:walletconnect-pairing", { detail: payload ?? {} })
        );
      }
    };
})();
