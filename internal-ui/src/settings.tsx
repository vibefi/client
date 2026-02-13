import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { IpcClient } from "./ipc/client";
import { PROVIDER_IDS } from "./ipc/contracts";
import {
  composeStyles,
  sharedFeedbackStyles,
  sharedFormFieldStyles,
  sharedPageStyles,
  sharedStyles,
  sharedSurfaceStyles,
  sharedUtilityStyles,
} from "./styles/shared";

declare global {
  interface Window {
    __WryEthereumResolve?: (id: number, result: unknown, error: unknown) => void;
  }
}

type RpcEndpoint = {
  url: string;
  label?: string;
};

type IpfsFetchBackend = "helia" | "localnode";

type IpfsSettings = {
  fetchBackend: IpfsFetchBackend;
  gatewayEndpoint: string;
  defaultGatewayEndpoint: string;
};

const DEFAULT_IPFS_SETTINGS: IpfsSettings = {
  fetchBackend: "helia",
  gatewayEndpoint: "http://127.0.0.1:8080",
  defaultGatewayEndpoint: "http://127.0.0.1:8080",
};

const localStyles = `
  .section { margin-bottom: 28px; }
  .section h2 { font-size: 16px; margin-bottom: 12px; }
  .endpoint-list { display: flex; flex-direction: column; gap: 8px; margin-bottom: 16px; }
  .endpoint-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
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
  .radio-group { display: flex; flex-direction: column; gap: 8px; margin-bottom: 12px; }
  .radio-option {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    padding: 10px 12px;
  }
  .radio-option input { margin-top: 2px; }
  .radio-option .label { font-size: 13px; font-weight: 600; color: #1e293b; }
  .radio-option .desc { font-size: 12px; color: #64748b; margin-top: 2px; }
  .muted { font-size: 12px; color: #64748b; margin-top: 6px; }
  .ipfs-actions { margin-top: 12px; display: flex; gap: 8px; }
  button.primary:disabled { opacity: 0.5; cursor: default; }
`;
const styles = composeStyles(
  sharedStyles,
  sharedPageStyles,
  sharedFormFieldStyles,
  sharedFeedbackStyles,
  sharedSurfaceStyles,
  sharedUtilityStyles,
  localStyles
);

const settingsClient = new IpcClient();

window.__WryEthereumResolve = (id: number, result: unknown, error: unknown) => {
  settingsClient.resolve(id, result, error);
};

function settingsIpc(method: string, params: unknown[] = []): Promise<unknown> {
  return settingsClient.request(PROVIDER_IDS.settings, method, params);
}

function parseIpfsSettings(value: unknown): IpfsSettings {
  if (!value || typeof value !== "object") return DEFAULT_IPFS_SETTINGS;
  const record = value as Record<string, unknown>;
  const backend = record.fetchBackend === "localnode" ? "localnode" : "helia";
  const defaultGateway =
    typeof record.defaultGatewayEndpoint === "string" && record.defaultGatewayEndpoint.trim()
      ? record.defaultGatewayEndpoint.trim()
      : DEFAULT_IPFS_SETTINGS.defaultGatewayEndpoint;
  const gateway =
    typeof record.gatewayEndpoint === "string" && record.gatewayEndpoint.trim()
      ? record.gatewayEndpoint.trim()
      : defaultGateway;
  return {
    fetchBackend: backend,
    gatewayEndpoint: gateway,
    defaultGatewayEndpoint: defaultGateway,
  };
}

function App() {
  const [endpoints, setEndpoints] = useState<RpcEndpoint[]>([]);
  const [newUrl, setNewUrl] = useState("");
  const [newLabel, setNewLabel] = useState("");
  const [status, setStatus] = useState<{ text: string; ok: boolean } | null>(null);
  const [loadingEndpoints, setLoadingEndpoints] = useState(true);
  const [loadingIpfs, setLoadingIpfs] = useState(true);
  const [ipfsDraft, setIpfsDraft] = useState<IpfsSettings>(DEFAULT_IPFS_SETTINGS);
  const [savingIpfs, setSavingIpfs] = useState(false);

  useEffect(() => {
    void Promise.all([loadEndpoints(), loadIpfsSettings()]);
  }, []);

  const loadEndpoints = async () => {
    setLoadingEndpoints(true);
    try {
      const result = await settingsIpc("vibefi_getEndpoints");
      setEndpoints(Array.isArray(result) ? (result as RpcEndpoint[]) : []);
    } catch {
      setEndpoints([]);
    } finally {
      setLoadingEndpoints(false);
    }
  };

  const loadIpfsSettings = async () => {
    setLoadingIpfs(true);
    try {
      const result = await settingsIpc("vibefi_getIpfsSettings");
      setIpfsDraft(parseIpfsSettings(result));
    } catch {
      setIpfsDraft(DEFAULT_IPFS_SETTINGS);
    } finally {
      setLoadingIpfs(false);
    }
  };

  const saveEndpoints = async (next: RpcEndpoint[]) => {
    try {
      await settingsIpc("vibefi_setEndpoints", [next]);
      setEndpoints(next);
      setStatus({ text: "Saved", ok: true });
    } catch (err: any) {
      setStatus({ text: err?.message || String(err), ok: false });
    }
  };

  const saveIpfsSettings = async () => {
    setSavingIpfs(true);
    try {
      await settingsIpc("vibefi_setIpfsSettings", [{
        fetchBackend: ipfsDraft.fetchBackend,
        gatewayEndpoint: ipfsDraft.fetchBackend === "localnode" ? ipfsDraft.gatewayEndpoint.trim() : undefined,
      }]);
      setStatus({ text: "Saved", ok: true });
    } catch (err: any) {
      setStatus({ text: err?.message || String(err), ok: false });
    } finally {
      setSavingIpfs(false);
    }
  };

  const addEndpoint = () => {
    const url = newUrl.trim();
    if (!url) return;
    const next = [...endpoints, { url, label: newLabel.trim() || undefined }];
    setNewUrl("");
    setNewLabel("");
    void saveEndpoints(next);
  };

  const removeEndpoint = (idx: number) => {
    if (idx === 0) return;
    const next = endpoints.filter((_, i) => i !== idx);
    void saveEndpoints(next);
  };

  const moveUp = (idx: number) => {
    if (idx <= 1) return;
    const next = [...endpoints];
    [next[idx - 1], next[idx]] = [next[idx], next[idx - 1]];
    void saveEndpoints(next);
  };

  const moveDown = (idx: number) => {
    if (idx === 0 || idx >= endpoints.length - 1) return;
    const next = [...endpoints];
    [next[idx], next[idx + 1]] = [next[idx + 1], next[idx]];
    void saveEndpoints(next);
  };

  return (
    <>
      <style>{styles}</style>
      <div className="page-container wide">
        <h1 className="page-title">Settings</h1>
        <div className="subtitle">Manage RPC endpoints and IPFS retrieval preferences.</div>

        <div className="section">
          <h2>RPC Endpoints</h2>
          {loadingEndpoints ? (
            <div className="empty">Loading...</div>
          ) : endpoints.length === 0 ? (
            <div className="empty">No endpoints configured.</div>
          ) : (
            <div className="endpoint-list">
              {endpoints.map((ep, idx) => (
                <div className="endpoint-item surface-card" key={`${idx}-${ep.url}`}>
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
            <div className="field flex-2">
              <label>URL</label>
              <input
                type="text"
                placeholder="https://rpc.example.com"
                value={newUrl}
                onChange={(e) => setNewUrl(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter") addEndpoint(); }}
              />
            </div>
            <div className="field flex-1">
              <label>Label (optional)</label>
              <input
                type="text"
                placeholder="My RPC"
                value={newLabel}
                onChange={(e) => setNewLabel(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter") addEndpoint(); }}
              />
            </div>
            <button className="secondary mb-0" onClick={addEndpoint}>Add</button>
          </div>
        </div>

        <div className="section">
          <h2>IPFS Retrieval</h2>
          {loadingIpfs ? (
            <div className="empty">Loading...</div>
          ) : (
            <>
              <div className="radio-group">
                <label className="radio-option surface-card">
                  <input
                    type="radio"
                    name="ipfs-backend"
                    checked={ipfsDraft.fetchBackend === "helia"}
                    onChange={() => setIpfsDraft((curr) => ({ ...curr, fetchBackend: "helia" }))}
                  />
                  <div>
                    <div className="label">Helia Verified Fetch (Recommended)</div>
                    <div className="desc">Fetches via trustless gateways with local cryptographic verification. No local node needed.</div>
                  </div>
                </label>
                <label className="radio-option surface-card">
                  <input
                    type="radio"
                    name="ipfs-backend"
                    checked={ipfsDraft.fetchBackend === "localnode"}
                    onChange={() => setIpfsDraft((curr) => ({ ...curr, fetchBackend: "localnode" }))}
                  />
                  <div>
                    <div className="label">Local IPFS Node</div>
                    <div className="desc">For advanced users running their own IPFS daemon. Implicitly trusted.</div>
                  </div>
                </label>
              </div>

              {ipfsDraft.fetchBackend === "localnode" ? (
                <div className="field">
                  <label>Local Node Endpoint</label>
                  <input
                    type="text"
                    placeholder="http://127.0.0.1:8080"
                    value={ipfsDraft.gatewayEndpoint}
                    onChange={(e) => setIpfsDraft((curr) => ({ ...curr, gatewayEndpoint: e.target.value }))}
                  />
                </div>
              ) : null}

              <div className="ipfs-actions">
                <button className="primary" onClick={() => void saveIpfsSettings()} disabled={savingIpfs}>
                  {savingIpfs ? "Saving..." : "Save IPFS Settings"}
                </button>
              </div>
            </>
          )}
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
