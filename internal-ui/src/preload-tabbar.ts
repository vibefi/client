(() => {
  (window as any).__VibefiTabbarState = (window as any).__VibefiTabbarState || null;
  (window as any).__VibefiHostDispatch =
    (window as any).__VibefiHostDispatch ||
    function (message: any) {
      if (!message || typeof message !== "object") return;
      const kind = message.kind;
      const payload = message.payload;
      if (kind === "tabbarUpdate" && typeof (window as any).updateTabs === "function" && payload) {
        (window as any).__VibefiTabbarState = payload;
        (window as any).updateTabs(payload.tabs ?? [], payload.activeIndex ?? 0);
        return;
      }
      if (kind === "tabbarUpdate" && payload) {
        (window as any).__VibefiTabbarState = payload;
      }
    };
})();
