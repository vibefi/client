import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";

declare global {
  interface Window {
    ipc: {
      postMessage: (message: string) => void;
    };
  }
}

type PairingPayload = {
  uri?: string;
  qrSvg?: string;
};

const styles = `
  :root { color-scheme: dark; }
  * { margin: 0; padding: 0; box-sizing: border-box; }
  html, body {
    height: 100%;
    overflow: hidden;
    background: rgba(2, 6, 23, 0.96);
    color: #e2e8f0;
    font-family: -apple-system, BlinkMacSystemFont, system-ui, sans-serif;
    font-size: 12px;
    line-height: 1.4;
  }
  #root { height: 100%; }
  .panel {
    padding: 12px;
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }
  .header strong { font-size: 13px; }
  button {
    border: 1px solid #475569;
    background: #0f172a;
    color: #e2e8f0;
    border-radius: 8px;
    padding: 4px 8px;
    cursor: pointer;
    font-size: 12px;
  }
  button:hover { background: #1e293b; }
  .desc { opacity: 0.9; margin-bottom: 8px; }
  .qr {
    display: flex;
    justify-content: center;
    margin-bottom: 8px;
    flex-shrink: 0;
  }
  .qr svg { max-width: 200px; max-height: 200px; }
  textarea {
    width: 100%;
    height: 80px;
    background: #020617;
    color: #93c5fd;
    border: 1px solid #1e293b;
    border-radius: 8px;
    padding: 8px;
    resize: none;
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
    font-size: 11px;
  }
  .footer {
    display: flex;
    justify-content: flex-end;
    margin-top: 8px;
  }
  .copy-btn { padding: 6px 10px; }
`;

function emitHideOverlay() {
  window.ipc.postMessage(
    JSON.stringify({ providerId: "vibefi-wallet", method: "hideOverlay", params: [] })
  );
}

async function copyText(value: string) {
  if (!value) return;
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(value);
      return;
    }
  } catch {
    // ignored
  }

  const el = document.getElementById("uri") as HTMLTextAreaElement | null;
  if (!el) return;
  el.focus();
  el.select();
  document.execCommand("copy");
}

function App() {
  const [uri, setUri] = useState("");
  const [qrSvg, setQrSvg] = useState("");

  useEffect(() => {
    const onPairing = (event: Event) => {
      const custom = event as CustomEvent<PairingPayload>;
      const detail = custom.detail || {};
      setUri(typeof detail.uri === "string" ? detail.uri : "");
      setQrSvg(typeof detail.qrSvg === "string" ? detail.qrSvg : "");
    };

    window.addEventListener("vibefi:walletconnect-pairing", onPairing);
    return () => {
      window.removeEventListener("vibefi:walletconnect-pairing", onPairing);
    };
  }, []);

  return (
    <>
      <style>{styles}</style>
      <div className="panel">
        <div className="header">
          <strong>WalletConnect Pairing</strong>
          <button onClick={emitHideOverlay}>Hide</button>
        </div>
        <div className="desc">
          Open a WalletConnect-compatible wallet and approve the session. You can copy the pairing URI below.
        </div>
        <div className="qr" dangerouslySetInnerHTML={{ __html: qrSvg }} />
        <textarea id="uri" value={uri} readOnly />
        <div className="footer">
          <button className="copy-btn" onClick={() => void copyText(uri)}>Copy URI</button>
        </div>
      </div>
    </>
  );
}

const rootEl = document.getElementById("root");
if (rootEl) {
  createRoot(rootEl).render(<App />);
}
