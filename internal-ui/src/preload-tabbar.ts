import { handleHostDispatch } from "./ipc/host-dispatch";

declare global {
  interface Window {
    __VibefiTabbarState?: unknown;
    updateTabs?: (tabs: unknown[], activeIndex: number) => void;
    updateRpcStatus?: (webviewId: string, pendingCount: number) => void;
    __VibefiHostDispatch?: (message: unknown) => void;
  }
}

(() => {
  window.__VibefiTabbarState = window.__VibefiTabbarState || null;
  window.__VibefiHostDispatch =
    window.__VibefiHostDispatch ||
    function (message: unknown) {
      handleHostDispatch(message, {
        onTabbarUpdate: (payload) => {
          window.__VibefiTabbarState = payload;
          if (typeof window.updateTabs === "function") {
            window.updateTabs(payload.tabs ?? [], payload.activeIndex ?? 0);
          }
        },
        onRpcStatus: (payload) => {
          if (typeof window.updateRpcStatus === "function") {
            window.updateRpcStatus(payload.webviewId ?? "", payload.pendingCount ?? 0);
          }
        },
      });
    };
})();
