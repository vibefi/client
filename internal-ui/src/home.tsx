import React from "react";
import { createRoot } from "react-dom/client";

type Eip1193Request = (args: { method: string; params?: unknown[] }) => Promise<unknown>;
type Eip1193Provider = {
  request?: Eip1193Request;
  on?: (event: string, listener: (...args: unknown[]) => void) => void;
};

declare global {
  interface Window {
    ethereum?: Eip1193Provider;
  }
}

const styles = `
  * { box-sizing: border-box; }
  body { font-family: system-ui, sans-serif; margin: 24px; }
  pre {
    background: #f6f6f6;
    padding: 12px;
    border-radius: 12px;
    overflow: auto;
    min-height: 160px;
    white-space: pre-wrap;
  }
  button {
    padding: 10px 14px;
    border-radius: 12px;
    border: 1px solid #ddd;
    margin-right: 8px;
    margin-bottom: 8px;
    cursor: pointer;
  }
`;

function stringify(value: unknown): string {
  return typeof value === "string" ? value : JSON.stringify(value, null, 2);
}

function asErrorMessage(error: unknown): string {
  if (error && typeof error === "object" && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string") return message;
  }
  return String(error);
}

function App() {
  const [lines, setLines] = React.useState<string[]>([]);

  const log = React.useCallback((value: unknown) => {
    setLines((prev) => [...prev, stringify(value)]);
  }, []);

  const ensureProvider = React.useCallback((): Eip1193Provider => {
    if (!window.ethereum) {
      throw new Error("window.ethereum is not injected");
    }
    if (!window.ethereum.request) {
      throw new Error("window.ethereum.request is missing");
    }
    return window.ethereum;
  }, []);

  const run = React.useCallback(
    async (label: string, fn: () => Promise<unknown>) => {
      try {
        log(`> ${label}`);
        const result = await fn();
        log(result);
      } catch (error) {
        log({ error: asErrorMessage(error) });
      }
      log("");
    },
    [log]
  );

  React.useEffect(() => {
    const provider = window.ethereum;
    if (!provider?.on) return;
    provider.on("accountsChanged", (accs) => log({ event: "accountsChanged", accs }));
    provider.on("chainChanged", (cid) => log({ event: "chainChanged", cid }));
    provider.on("connect", (info) => log({ event: "connect", info }));
  }, [log]);

  return (
    <>
      <style>{styles}</style>
      <h1>Wry + EIP-1193 Provider</h1>
      <p>Open the developer console to see provider logs. This page calls window.ethereum.request.</p>

      <div>
        <button
          onClick={() =>
            void run("eth_requestAccounts", async () => {
              const provider = ensureProvider();
              return await provider.request!({ method: "eth_requestAccounts", params: [] });
            })
          }
        >
          eth_requestAccounts
        </button>
        <button
          onClick={() =>
            void run("eth_chainId", async () => {
              const provider = ensureProvider();
              return await provider.request!({ method: "eth_chainId", params: [] });
            })
          }
        >
          eth_chainId
        </button>
        <button
          onClick={() =>
            void run("personal_sign", async () => {
              const provider = ensureProvider();
              const msg = "Hello from Wry";
              const accounts = (await provider.request!({
                method: "eth_requestAccounts",
                params: [],
              })) as string[];
              return await provider.request!({
                method: "personal_sign",
                params: [msg, accounts[0]],
              });
            })
          }
        >
          personal_sign
        </button>
        <button
          onClick={() =>
            void run("eth_signTypedData_v4", async () => {
              const provider = ensureProvider();
              const accounts = (await provider.request!({
                method: "eth_requestAccounts",
                params: [],
              })) as string[];
              const from = accounts[0];
              const typed = {
                types: {
                  EIP712Domain: [
                    { name: "name", type: "string" },
                    { name: "version", type: "string" },
                    { name: "chainId", type: "uint256" },
                    { name: "verifyingContract", type: "address" },
                  ],
                  Mail: [
                    { name: "from", type: "address" },
                    { name: "to", type: "address" },
                    { name: "contents", type: "string" },
                  ],
                },
                primaryType: "Mail",
                domain: {
                  name: "Wry Demo",
                  version: "1",
                  chainId: 1,
                  verifyingContract: "0x0000000000000000000000000000000000000000",
                },
                message: {
                  from,
                  to: "0x0000000000000000000000000000000000000001",
                  contents: "Hello, EIP-712!",
                },
              };
              return await provider.request!({
                method: "eth_signTypedData_v4",
                params: [from, JSON.stringify(typed)],
              });
            })
          }
        >
          eth_signTypedData_v4
        </button>
      </div>

      <h3>Output</h3>
      <pre>{lines.join("\n")}</pre>
    </>
  );
}

const rootEl = document.getElementById("root");
if (rootEl) {
  createRoot(rootEl).render(<App />);
}
