import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { IpcClient } from "./ipc/client";
import { PROVIDER_IDS, type Tab } from "./ipc/contracts";
import { composeStyles, sharedStyles } from "./styles/shared";

declare global {
  interface Window {
    updateTabs?: (tabs: unknown[], activeIndex: number) => void;
    __VibefiTabbarState?: unknown;
  }
}

const tabbarClient = new IpcClient();

const localStyles = `
* { margin: 0; padding: 0; }
html, body {
  height: 40px;
  overflow: hidden;
  background: #0f172a;
  color: #e2e8f0;
  font-family: -apple-system, BlinkMacSystemFont, system-ui, sans-serif;
  font-size: 13px;
  -webkit-user-select: none;
  user-select: none;
}
#root { height: 40px; }
#tabs {
  display: flex;
  align-items: center;
  height: 40px;
  padding: 0 4px;
  gap: 2px;
}
.tab {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 12px;
  border-radius: 6px 6px 0 0;
  cursor: pointer;
  white-space: nowrap;
  max-width: 180px;
  border: 1px solid transparent;
  border-bottom: none;
  background: transparent;
  color: #94a3b8;
  transition: background .15s, color .15s;
}
.tab:hover { background: #1e293b; color: #e2e8f0; }
.tab.active { background: #1e293b; color: #e2e8f0; border-color: #334155; }
.tab.disabled { cursor: default; opacity: 0.9; }
.tab.disabled:hover { background: transparent; color: #94a3b8; }
.tab-label { overflow: hidden; text-overflow: ellipsis; }
.tab-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border-radius: 4px;
  font-size: 12px;
  line-height: 1;
  opacity: 0.5;
  cursor: pointer;
}
.tab-close:hover { opacity: 1; background: #334155; }
.tab-spinner {
  width: 12px;
  height: 12px;
  border: 2px solid rgba(148, 163, 184, 0.35);
  border-top-color: #e2e8f0;
  border-radius: 50%;
  animation: tab-spin 0.8s linear infinite;
}
@keyframes tab-spin { to { transform: rotate(360deg); } }
`;
const styles = composeStyles(sharedStyles, localStyles);

function postTabbarCommand(method: "switchTab" | "closeTab", index: number) {
  tabbarClient.notify(PROVIDER_IDS.tabbar, method, [index]);
}

function App() {
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeIndex, setActiveIndex] = useState(0);

  useEffect(() => {
    window.updateTabs = (nextTabs: unknown[], nextActiveIndex: number) => {
      setTabs(Array.isArray(nextTabs) ? (nextTabs as Tab[]) : []);
      setActiveIndex(Number.isFinite(nextActiveIndex) ? nextActiveIndex : 0);
    };

    const initial = window.__VibefiTabbarState as
      | { tabs?: unknown[]; activeIndex?: number }
      | undefined;
    if (initial && typeof window.updateTabs === "function") {
      window.updateTabs(initial.tabs ?? [], initial.activeIndex ?? 0);
    }

    return () => {
      delete window.updateTabs;
    };
  }, []);

  return (
    <>
      <style>{styles}</style>
      <div id="tabs">
        {tabs.map((tab, index) => (
          <div
            key={`${tab.id ?? "tab"}:${index}`}
            className={`tab${index === activeIndex ? " active" : ""}${tab.clickable === false ? " disabled" : ""}`}
            onClick={() => {
              if (tab.clickable === false) return;
              postTabbarCommand("switchTab", index);
            }}
          >
            <span className="tab-label">{tab.label || tab.id || "Tab"}</span>
            {tab.loading ? <span className="tab-spinner" aria-label="loading" /> : null}
            {tabs.length > 1 && tab.closable !== false ? (
              <span
                className="tab-close"
                onClick={(event) => {
                  event.stopPropagation();
                  postTabbarCommand("closeTab", index);
                }}
              >
                &times;
              </span>
            ) : null}
          </div>
        ))}
      </div>
    </>
  );
}

const rootEl = document.getElementById("root");
if (rootEl) {
  createRoot(rootEl).render(<App />);
}
