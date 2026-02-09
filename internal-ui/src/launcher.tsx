import React, { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";

type DappInfo = {
  dappId: string;
  versionId: string;
  name: string;
  version: string;
  description: string;
  status: string;
  rootCid: string;
};

type VibefiRequest = (args: { method: string; params?: unknown[] }) => Promise<unknown>;

declare global {
  interface Window {
    vibefi?: {
      request?: VibefiRequest;
    };
  }
}

const styles = `
  :root { color-scheme: light; }
  * { box-sizing: border-box; }
  body {
    font-family: system-ui, sans-serif;
    margin: 0;
    color: #0f172a;
    background: #f8fafc;
  }
  .app { padding: 24px; }
  h1 { margin: 0 0 8px; font-size: 26px; }
  p { margin: 0 0 16px; color: #475569; }
  .row { display: flex; gap: 8px; margin-bottom: 16px; }
  .notice {
    margin-bottom: 16px;
    padding: 10px 12px;
    border-radius: 10px;
    background: #fff7ed;
    border: 1px solid #fed7aa;
    color: #9a3412;
    font-size: 14px;
  }
  button {
    padding: 10px 14px;
    border-radius: 10px;
    border: 1px solid #cbd5f5;
    background: #fff;
    cursor: pointer;
  }
  button:disabled { opacity: 0.6; cursor: default; }
  button.primary { background: #0f172a; color: #fff; border-color: #0f172a; }
  table {
    width: 100%;
    border-collapse: collapse;
    background: #fff;
    border-radius: 12px;
    overflow: hidden;
  }
  th, td {
    text-align: left;
    padding: 10px 12px;
    border-bottom: 1px solid #e2e8f0;
    font-size: 14px;
  }
  th { background: #f1f5f9; color: #0f172a; font-weight: 600; }
  tr:hover td { background: #f8fafc; }
  .dapp-row.unavailable td { color: #64748b; }
  .dapp-row.unavailable { opacity: 0.62; }
  .pill {
    display: inline-block;
    margin-left: 8px;
    padding: 2px 7px;
    border-radius: 9999px;
    font-size: 12px;
    border: 1px solid #fed7aa;
    background: #fff7ed;
    color: #9a3412;
    font-weight: 600;
  }
  .log {
    margin-top: 16px;
    background: #0f172a;
    color: #e2e8f0;
    padding: 12px;
    border-radius: 12px;
    min-height: 120px;
    white-space: pre-wrap;
    font-size: 12px;
    max-height: 220px;
    overflow: auto;
  }
`;

function asErrorMessage(err: unknown): string {
  if (err && typeof err === "object" && "message" in err && typeof (err as { message?: unknown }).message === "string") {
    return (err as { message: string }).message;
  }
  return String(err);
}

function App() {
  const [items, setItems] = useState<DappInfo[]>([]);
  const [selectedIndex, setSelectedIndex] = useState<number | null>(null);
  const [logs, setLogs] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);

  const selectedItem = useMemo(() => {
    if (selectedIndex === null) return null;
    return items[selectedIndex] ?? null;
  }, [items, selectedIndex]);
  const pausedItems = useMemo(
    () => items.filter((item) => item.status === "Paused"),
    [items]
  );

  const addLog = (line: string) => {
    setLogs((prev) => [...prev, line]);
  };

  const vibefiRequest = async (method: string, params: unknown[] = []) => {
    const request = window.vibefi?.request;
    if (!request) {
      throw new Error("vibefi request API not available");
    }
    return await request({ method, params });
  };

  const refresh = async () => {
    setBusy(true);
    addLog("Fetching dapp list...");
    try {
      const result = await vibefiRequest("vibefi_listDapps", []);
      const nextItems = Array.isArray(result) ? (result as DappInfo[]) : [];
      setItems(nextItems);
      setSelectedIndex(null);
      addLog(`Found ${nextItems.length} dapps.`);
    } catch (err) {
      addLog(`Error: ${asErrorMessage(err)}`);
    } finally {
      setBusy(false);
    }
  };

  const launch = async () => {
    if (!selectedItem) return;
    if (selectedItem.status !== "Published") {
      addLog(`Cannot launch ${selectedItem.name || selectedItem.rootCid}: app is ${selectedItem.status}.`);
      return;
    }
    setBusy(true);
    addLog(`Launching ${selectedItem.name || ""} ${selectedItem.version || ""} (${selectedItem.rootCid})`);
    try {
      await vibefiRequest("vibefi_launchDapp", [selectedItem.rootCid, selectedItem.name || selectedItem.rootCid]);
      addLog("Launch request sent.");
    } catch (err) {
      addLog(`Error: ${asErrorMessage(err)}`);
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  return (
    <>
      <style>{styles}</style>
      <div className="app">
        <h1>vibe.fi devnet</h1>
        <p>Pick a published vapp to fetch, verify, build, and launch.</p>

        <div className="row">
          <button onClick={() => void refresh()} disabled={busy}>Refresh list</button>
          <button
            className="primary"
            onClick={() => void launch()}
            disabled={busy || !selectedItem || selectedItem.status !== "Published"}
          >
            Launch selected
          </button>
          <button onClick={() => void vibefiRequest("vibefi_openSettings")}>Settings</button>
        </div>
        <table>
          <thead>
            <tr>
              <th></th>
              <th>Dapp</th>
              <th>Version</th>
              <th>Root CID</th>
            </tr>
          </thead>
          <tbody>
            {items.length === 0 ? (
              <tr>
                <td colSpan={4}>No dapps found.</td>
              </tr>
            ) : (
              items.map((item, idx) => (
                <tr
                  key={`${item.dappId}:${item.versionId}:${item.rootCid}`}
                  className={`dapp-row ${item.status === "Published" ? "" : "unavailable"}`}
                >
                  <td>
                    <input
                      type="radio"
                      name="select"
                      checked={selectedIndex === idx}
                      onChange={() => setSelectedIndex(idx)}
                      disabled={item.status !== "Published"}
                    />
                  </td>
                  <td>
                    {item.name || "(unnamed)"} #{item.dappId}
                    {item.status === "Paused" && <span className="pill">Paused</span>}
                  </td>
                  <td>{item.version || `v${item.versionId || ""}`}</td>
                  <td>{item.rootCid || ""}</td>
                </tr>
              ))
            )}
          </tbody>
        </table>

        <pre className="log">{logs.join("\n")}</pre>
      </div>
    </>
  );
}

const rootEl = document.getElementById("root");
if (rootEl) {
  createRoot(rootEl).render(<App />);
}
