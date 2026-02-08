import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { IpcClient } from "./ipc/client";
import { PROVIDER_IDS } from "./ipc/contracts";

declare global {
  interface Window {
    __WryEthereumResolve?: (id: number, result: unknown, error: unknown) => void;
  }
}

type RpcEndpoint = {
  url: string;
  label?: string;
};

const styles = `
  :root { color-scheme: light; }
  * { box-sizing: border-box; margin: 0; padding: 0; }
  body {
    font-family: system-ui, -apple-system, BlinkMacSystemFont, sans-serif;
    background: #f8fafc;
    color: #0f172a;
  }
  .container {
    max-width: 560px;
    margin: 40px auto;
    padding: 32px;
  }
  h1 { font-size: 22px; margin-bottom: 6px; }
  .subtitle { color: #475569; margin-bottom: 24px; font-size: 14px; }
  .section { margin-bottom: 24px; }
  .section h2 { font-size: 16px; margin-bottom: 12px; }
  .endpoint-list { display: flex; flex-direction: column; gap: 8px; margin-bottom: 16px; }
  .endpoint-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
    border: 1px solid #e2e8f0;
    border-radius: 10px;
    background: #fff;
    font-size: 13px;
  }
  .endpoint-item .index {
    width: 22px; height: 22px;
    border-radius: 6px;
    background: #f1f5f9;
    display: flex; align-items: center; justify-content: center;
    font-size: 11px; font-weight: 600; color: #64748b;
    flex-shrink: 0;
  }
  .endpoint-item .info { flex: 1; min-width: 0; }
  .endpoint-item .url {
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
    font-size: 12px;
    color: #334155;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .endpoint-item .lbl { font-size: 11px; color: #94a3b8; }
  .endpoint-item .default-badge {
    font-size: 10px;
    background: #dbeafe;
    color: #1d4ed8;
    padding: 2px 6px;
    border-radius: 4px;
    font-weight: 600;
  }
  .endpoint-actions { display: flex; gap: 4px; }
  .endpoint-actions button {
    width: 26px; height: 26px;
    border: 1px solid #e2e8f0;
    border-radius: 6px;
    background: #fff;
    cursor: pointer;
    font-size: 12px;
    display: flex; align-items: center; justify-content: center;
    color: #64748b;
  }
  .endpoint-actions button:hover { background: #f1f5f9; }
  .endpoint-actions button:disabled { opacity: 0.3; cursor: default; }
  .add-form {
    display: flex;
    gap: 8px;
    align-items: flex-end;
  }
  .add-form .field { flex: 1; }
  .add-form label { display: block; font-size: 12px; color: #64748b; margin-bottom: 4px; }
  .add-form input {
    width: 100%;
    padding: 8px 10px;
    border: 1px solid #e2e8f0;
    border-radius: 8px;
    font-size: 13px;
    background: #fff;
  }
  .add-form input:focus { outline: none; border-color: #94a3b8; }
  button.primary {
    padding: 8px 16px;
    border-radius: 8px;
    border: 1px solid #0f172a;
    background: #0f172a;
    color: #fff;
    cursor: pointer;
    font-size: 13px;
  }
  button.primary:hover { background: #1e293b; }
  button.primary:disabled { opacity: 0.5; cursor: default; }
  button.secondary {
    padding: 8px 16px;
    border-radius: 8px;
    border: 1px solid #cbd5e1;
    background: #fff;
    cursor: pointer;
    font-size: 13px;
  }
  button.secondary:hover { background: #f1f5f9; }
  .bottom-actions { display: flex; gap: 8px; }
  .status { font-size: 13px; margin-top: 8px; }
  .status.ok { color: #0f766e; }
  .status.err { color: #dc2626; }
  .empty { color: #94a3b8; font-size: 13px; padding: 12px 0; }
`;

const settingsClient = new IpcClient();

window.__WryEthereumResolve = (id: number, result: unknown, error: unknown) => {
  settingsClient.resolve(id, result, error);
};

function settingsIpc(method: string, params: unknown[] = []): Promise<unknown> {
  return settingsClient.request(PROVIDER_IDS.settings, method, params);
}

function App() {
  const [endpoints, setEndpoints] = useState<RpcEndpoint[]>([]);
  const [newUrl, setNewUrl] = useState("");
  const [newLabel, setNewLabel] = useState("");
  const [status, setStatus] = useState<{ text: string; ok: boolean } | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    void loadEndpoints();
  }, []);

  const loadEndpoints = async () => {
    setLoading(true);
    try {
      const result = await settingsIpc("vibefi_getEndpoints");
      setEndpoints(Array.isArray(result) ? (result as RpcEndpoint[]) : []);
    } catch {
      setEndpoints([]);
    } finally {
      setLoading(false);
    }
  };

  const save = async (next: RpcEndpoint[]) => {
    try {
      await settingsIpc("vibefi_setEndpoints", [next]);
      setEndpoints(next);
      setStatus({ text: "Saved", ok: true });
    } catch (err: any) {
      setStatus({ text: err?.message || String(err), ok: false });
    }
  };

  const addEndpoint = () => {
    const url = newUrl.trim();
    if (!url) return;
    const next = [...endpoints, { url, label: newLabel.trim() || undefined }];
    setNewUrl("");
    setNewLabel("");
    void save(next);
  };

  const removeEndpoint = (idx: number) => {
    if (idx === 0) return; // can't remove default
    const next = endpoints.filter((_, i) => i !== idx);
    void save(next);
  };

  const moveUp = (idx: number) => {
    if (idx <= 1) return; // can't move above default
    const next = [...endpoints];
    [next[idx - 1], next[idx]] = [next[idx], next[idx - 1]];
    void save(next);
  };

  const moveDown = (idx: number) => {
    if (idx === 0 || idx >= endpoints.length - 1) return;
    const next = [...endpoints];
    [next[idx], next[idx + 1]] = [next[idx + 1], next[idx]];
    void save(next);
  };

  return (
    <>
      <style>{styles}</style>
      <div className="container">
        <h1>Settings</h1>
        <div className="subtitle">Manage RPC endpoints and other preferences.</div>

        <div className="section">
          <h2>RPC Endpoints</h2>
          {loading ? (
            <div className="empty">Loading...</div>
          ) : endpoints.length === 0 ? (
            <div className="empty">No endpoints configured.</div>
          ) : (
            <div className="endpoint-list">
              {endpoints.map((ep, idx) => (
                <div className="endpoint-item" key={`${idx}-${ep.url}`}>
                  <div className="index">{idx + 1}</div>
                  <div className="info">
                    <div className="url">{ep.url}</div>
                    {ep.label && <div className="lbl">{ep.label}</div>}
                  </div>
                  {idx === 0 && <span className="default-badge">DEFAULT</span>}
                  <div className="endpoint-actions">
                    <button onClick={() => moveUp(idx)} disabled={idx <= 1} title="Move up">&#x25B2;</button>
                    <button onClick={() => moveDown(idx)} disabled={idx === 0 || idx >= endpoints.length - 1} title="Move down">&#x25BC;</button>
                    <button onClick={() => removeEndpoint(idx)} disabled={idx === 0} title="Remove">&#x2715;</button>
                  </div>
                </div>
              ))}
            </div>
          )}

          <div className="add-form">
            <div className="field" style={{ flex: 2 }}>
              <label>URL</label>
              <input
                type="text"
                placeholder="https://rpc.example.com"
                value={newUrl}
                onChange={(e) => setNewUrl(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter") addEndpoint(); }}
              />
            </div>
            <div className="field" style={{ flex: 1 }}>
              <label>Label (optional)</label>
              <input
                type="text"
                placeholder="My RPC"
                value={newLabel}
                onChange={(e) => setNewLabel(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter") addEndpoint(); }}
              />
            </div>
            <button className="secondary" onClick={addEndpoint} style={{ marginBottom: 0 }}>Add</button>
          </div>

          {status && <div className={`status ${status.ok ? "ok" : "err"}`}>{status.text}</div>}
        </div>
      </div>
    </>
  );
}

const rootEl = document.getElementById("root");
if (rootEl) {
  createRoot(rootEl).render(<App />);
}
