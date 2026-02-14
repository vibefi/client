import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { IpcClient } from "./ipc/client";
import { PROVIDER_IDS, type WalletconnectPairingPayload } from "./ipc/contracts";
import {
  composeStyles,
  sharedFeedbackStyles,
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

type Phase = "select" | "connecting" | "done";

const localStyles = `
  .options { display: flex; flex-direction: column; gap: 12px; }
  .option {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 16px;
    border-radius: 12px;
    cursor: pointer;
    transition: border-color 0.15s, box-shadow 0.15s;
  }
  .option:hover { border-color: #94a3b8; box-shadow: 0 1px 4px rgba(0,0,0,0.06); }
  .option-icon {
    width: 40px; height: 40px;
    border-radius: 10px;
    display: flex; align-items: center; justify-content: center;
    font-size: 20px; flex-shrink: 0;
  }
  .option-icon.local { background: #dbeafe; }
  .option-icon.wc { background: #ede9fe; }
  .option-icon.hw { background: #d1fae5; }
  .option-text strong { display: block; font-size: 15px; margin-bottom: 2px; }
  .option-text span { font-size: 13px; color: #64748b; }

  .connecting-view { text-align: center; }
  .connecting-view h2 { font-size: 18px; margin-bottom: 8px; }
  .connecting-view .desc { color: #475569; font-size: 14px; margin-bottom: 20px; }
  .spinner {
    display: inline-block;
    width: 28px; height: 28px;
    border: 3px solid #e2e8f0;
    border-top-color: #3b82f6;
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
    margin-bottom: 12px;
  }
  @keyframes spin { to { transform: rotate(360deg); } }

  .qr-container {
    display: flex;
    justify-content: center;
    margin-bottom: 16px;
  }
  .qr-container svg { max-width: 240px; max-height: 240px; }
  textarea {
    width: 100%;
    height: 64px;
    background: #f1f5f9;
    color: #334155;
    border: 1px solid #e2e8f0;
    border-radius: 8px;
    padding: 8px;
    resize: none;
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
    font-size: 11px;
    margin-bottom: 8px;
  }
  .actions { display: flex; gap: 8px; justify-content: center; }

  .done-view { text-align: center; padding-top: 40px; }
  .done-view .check { font-size: 48px; margin-bottom: 12px; }
  .done-view h2 { font-size: 18px; margin-bottom: 4px; }
  .done-view .desc { color: #475569; font-size: 14px; }
`;
const styles = composeStyles(
  sharedStyles,
  sharedPageStyles,
  sharedFeedbackStyles,
  sharedSurfaceStyles,
  sharedUtilityStyles,
  localStyles
);

const walletClient = new IpcClient();

// Rust emits rpcResponse through __WryEthereumResolve for selector requests.
window.__WryEthereumResolve = (id: number, result: unknown, error: unknown) => {
  walletClient.resolve(id, result, error);
};

function walletIpc(method: string, params: unknown[] = []): Promise<unknown> {
  return walletClient.request(PROVIDER_IDS.wallet, method, params);
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
  const [phase, setPhase] = useState<Phase>("select");
  const [error, setError] = useState("");
  const [uri, setUri] = useState("");
  const [qrSvg, setQrSvg] = useState("");

  useEffect(() => {
    const onPairing = (event: Event) => {
      const custom = event as CustomEvent<WalletconnectPairingPayload>;
      const detail = custom.detail || {};
      if (typeof detail.uri === "string") setUri(detail.uri);
      if (typeof detail.qrSvg === "string") setQrSvg(detail.qrSvg);
    };
    window.addEventListener("vibefi:walletconnect-pairing", onPairing);
    return () => {
      window.removeEventListener("vibefi:walletconnect-pairing", onPairing);
    };
  }, []);

  const connectLocal = async () => {
    setPhase("connecting");
    setError("");
    try {
      await walletIpc("vibefi_connectLocal");
      setPhase("done");
    } catch (err: any) {
      console.warn("[vibefi:wallet-selector] local connect failed", err);
      setError(err?.message || String(err));
      setPhase("select");
    }
  };

  const connectWalletConnect = async () => {
    setPhase("connecting");
    setError("");
    try {
      await walletIpc("vibefi_connectWalletConnect");
      setPhase("done");
    } catch (err: any) {
      console.warn("[vibefi:wallet-selector] walletconnect connect failed", err);
      setError(err?.message || String(err));
      setPhase("select");
    }
  };

  const connectHardware = async () => {
    setPhase("connecting");
    setError("");
    try {
      await walletIpc("vibefi_connectHardware");
      setPhase("done");
    } catch (err: any) {
      console.warn("[vibefi:wallet-selector] hardware connect failed", err);
      setError(err?.message || String(err));
      setPhase("select");
    }
  };

  if (phase === "done") {
    return (
      <>
        <style>{styles}</style>
        <div className="page-container compact done-view">
          <div className="check">&#x2705;</div>
          <h2>Connected</h2>
          <div className="desc">Wallet connected successfully. This tab will close automatically.</div>
        </div>
      </>
    );
  }

  if (phase === "connecting" && !uri) {
    return (
      <>
        <style>{styles}</style>
        <div className="page-container compact connecting-view">
          <div className="spinner" />
          <h2>Connecting...</h2>
          <div className="desc">Setting up wallet connection</div>
          {error && <div className="error">{error}</div>}
        </div>
      </>
    );
  }

  if (phase === "connecting" && uri) {
    return (
      <>
        <style>{styles}</style>
        <div className="page-container compact connecting-view">
          <h2>Scan QR Code</h2>
          <div className="desc">Open a WalletConnect-compatible wallet and scan the QR code below.</div>
          {qrSvg && (
            <div className="qr-container" dangerouslySetInnerHTML={{ __html: qrSvg }} />
          )}
          <textarea id="uri" value={uri} readOnly />
          <div className="actions">
            <button onClick={() => void copyText(uri)}>Copy URI</button>
            <button onClick={() => { setPhase("select"); setUri(""); setQrSvg(""); setError(""); }}>Back</button>
          </div>
          {error && <div className="error">{error}</div>}
        </div>
      </>
    );
  }

  return (
    <>
      <style>{styles}</style>
      <div className="page-container compact">
        <h1 className="page-title">Connect Wallet</h1>
        <div className="subtitle">Choose how you want to connect to this dapp.</div>
        {error && <div className="error mt-0 mb-12">{error}</div>}
        <div className="options">
          <div className="option surface-card" onClick={connectLocal}>
            <div className="option-icon local">&#x1F511;</div>
            <div className="option-text">
              <strong>Local Signer</strong>
              <span>Use the built-in dev key for signing transactions.</span>
            </div>
          </div>
          <div className="option surface-card" onClick={connectWalletConnect}>
            <div className="option-icon wc">&#x1F4F1;</div>
            <div className="option-text">
              <strong>WalletConnect</strong>
              <span>Connect a mobile wallet by scanning a QR code.</span>
            </div>
          </div>
          <div className="option surface-card" onClick={connectHardware}>
            <div className="option-icon hw">&#x1F50C;</div>
            <div className="option-text">
              <strong>Hardware Wallet</strong>
              <span>Connect a Ledger or Trezor device via USB.</span>
            </div>
          </div>
        </div>
      </div>
    </>
  );
}

const rootEl = document.getElementById("root");
if (rootEl) {
  createRoot(rootEl).render(<App />);
}
