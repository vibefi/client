import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";

type Tab = {
  id?: string;
  label?: string;
};

declare global {
  interface Window {
    ipc: {
      postMessage: (message: string) => void;
    };
    updateTabs?: (tabs: Tab[], activeIndex: number) => void;
    __VibefiTabbarState?: {
      tabs?: Tab[];
      activeIndex?: number;
    };
  }
}

const styles = `
* { margin: 0; padding: 0; box-sizing: border-box; }
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
`;

function postTabbarCommand(method: "switchTab" | "closeTab", index: number) {
  window.ipc.postMessage(
    JSON.stringify({
      providerId: "vibefi-tabbar",
      method,
      params: [index],
    })
  );
}

function App() {
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeIndex, setActiveIndex] = useState(0);

  useEffect(() => {
    window.updateTabs = (nextTabs: Tab[], nextActiveIndex: number) => {
      setTabs(Array.isArray(nextTabs) ? nextTabs : []);
      setActiveIndex(Number.isFinite(nextActiveIndex) ? nextActiveIndex : 0);
    };

    const initial = window.__VibefiTabbarState;
    if (initial) {
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
            className={`tab${index === activeIndex ? " active" : ""}`}
            onClick={() => postTabbarCommand("switchTab", index)}
          >
            <span className="tab-label">{tab.label || tab.id || "Tab"}</span>
            {tabs.length > 1 ? (
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
