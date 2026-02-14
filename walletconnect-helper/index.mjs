#!/usr/bin/env node

import process from "node:process";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import readline from "node:readline";
import { EthereumProvider } from "@walletconnect/ethereum-provider";
import QRCode from "qrcode";

// File-backed IKeyValueStorage implementation.
// The bundled WC SDK resolves to the browser entry which requires indexedDB.
// This provides a node-compatible storage that works in bun.
class FileKeyValueStorage {
  constructor(filePath) {
    this._path = filePath;
    this._data = {};
    try {
      const raw = fs.readFileSync(filePath, "utf8");
      this._data = JSON.parse(raw);
    } catch {}
  }
  _save() {
    const dir = path.dirname(this._path);
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(this._path, JSON.stringify(this._data), "utf8");
  }
  async getKeys() { return Object.keys(this._data); }
  async getEntries() { return Object.entries(this._data); }
  async getItem(key) { const v = this._data[key]; return v === undefined ? undefined : v; }
  async setItem(key, value) { this._data[key] = value; this._save(); }
  async removeItem(key) { delete this._data[key]; this._save(); }
}

const wcStoragePath = path.join(os.homedir(), ".vibefi", "walletconnect-store.json");
const wcStorage = new FileKeyValueStorage(wcStoragePath);

const projectId = process.env.VIBEFI_WC_PROJECT_ID || process.env.WC_PROJECT_ID || "";
const relayUrl = process.env.VIBEFI_WC_RELAY_URL || process.env.WC_RELAY_URL || undefined;
const metadataName = process.env.VIBEFI_WC_METADATA_NAME || "VibeFi Desktop";
const metadataUrl = process.env.VIBEFI_WC_METADATA_URL || "https://vibefi.local";
const metadataDesc = process.env.VIBEFI_WC_METADATA_DESC || "VibeFi desktop WalletConnect bridge";
const metadataIcon = process.env.VIBEFI_WC_METADATA_ICON || "";
const connectTimeoutMs = Number.parseInt(process.env.VIBEFI_WC_CONNECT_TIMEOUT_MS || "180000", 10);

// The WalletConnect SDK throws unhandled exceptions for several internal issues:
// - chainChanged fires before rpcProviders are populated (TypeError: setDefaultChain)
// - stale session topics from previous pairings (No matching key / session topic doesn't exist)
// These are non-fatal SDK bugs that shouldn't crash the helper process.
const WC_SDK_ERROR_PATTERNS = [
  /No matching key/,
  /session topic doesn't exist/,
  /isValidSessionTopic/,
];
// WC SDK fires internal async calls (chainChanged → setDefaultChain, switchEthereumChain,
// request) before rpcProviders are populated. These all manifest as TypeErrors from within
// the SDK's own call stack, not from our code.
const WC_SDK_STACK_MARKERS = [
  "@walletconnect/universal-provider",
  "@walletconnect/ethereum-provider",
  "@walletconnect/sign-client",
];
function isWcSdkError(err) {
  const msg = (err?.stack || err?.message || String(err));
  if (WC_SDK_ERROR_PATTERNS.some((p) => p.test(msg))) return true;
  // TypeErrors originating entirely within WC SDK internals (not our code)
  if (err instanceof TypeError && err.stack) {
    const frames = err.stack.split("\n").slice(1);
    const firstFrame = frames[0] || "";
    if (WC_SDK_STACK_MARKERS.some((m) => firstFrame.includes(m))) return true;
  }
  return false;
}
process.on("uncaughtException", (err) => {
  if (isWcSdkError(err)) {
    log(`suppressed WC SDK error: ${err.message}`);
    return;
  }
  log(`fatal uncaughtException: ${err.stack || err.message}`);
  process.exit(1);
});
process.on("unhandledRejection", (reason) => {
  if (isWcSdkError(reason)) {
    log(`suppressed WC SDK rejection: ${reason?.message || reason}`);
    return;
  }
  log(`fatal unhandledRejection: ${reason?.stack || reason}`);
  process.exit(1);
});

if (!projectId) {
  writeResponse({
    id: 0,
    error: {
      code: -32000,
      message:
        "WalletConnect project id missing. Set VIBEFI_WC_PROJECT_ID or use --wc-project-id."
    }
  });
  process.exit(1);
}

let provider = null;
let connectedAccounts = [];
let connectedChainIdHex = "0x1";

function writeMessage(payload) {
  process.stdout.write(`${JSON.stringify(payload)}\n`);
}

function writeResponse(payload) {
  writeMessage(payload);
}

function emitEvent(event, data = {}) {
  writeMessage({ event, ...data });
}

function log(message) {
  process.stderr.write(`[walletconnect-helper] ${message}\n`);
}

function normalizeChainIdHex(value) {
  if (typeof value === "number") return `0x${value.toString(16)}`;
  if (typeof value === "string") {
    if (value.startsWith("0x")) return value.toLowerCase();
    const parsed = Number.parseInt(value, 10);
    if (Number.isFinite(parsed)) return `0x${parsed.toString(16)}`;
  }
  return "0x1";
}

function uniqueNumbers(values) {
  const out = [];
  const seen = new Set();
  for (const value of values) {
    if (!Number.isFinite(value) || value <= 0) continue;
    if (seen.has(value)) continue;
    seen.add(value);
    out.push(value);
  }
  return out;
}

function parseAddressFromAccount(value) {
  if (typeof value !== "string") return null;
  if (value.startsWith("eip155:")) {
    const parts = value.split(":");
    return parts[2] || null;
  }
  if (value.startsWith("0x")) return value;
  return null;
}

function parseAccounts(result) {
  if (!Array.isArray(result)) return [];
  return result
    .map((entry) => parseAddressFromAccount(entry))
    .filter((entry) => typeof entry === "string");
}

async function ensureProvider(requiredChainId) {
  if (provider) return provider;
  const preferred = Number.isFinite(requiredChainId) ? Number(requiredChainId) : 1;
  const optionalChains = uniqueNumbers([preferred, 1, 11155111, 31337]);
  log(`init provider optionalChains=${optionalChains.join(",")}`);
  provider = await EthereumProvider.init({
    projectId,
    optionalChains,
    showQrModal: false,
    relayUrl,
    storage: wcStorage,
    metadata: {
      name: metadataName,
      description: metadataDesc,
      url: metadataUrl,
      icons: metadataIcon ? [metadataIcon] : []
    }
  });

  provider.on("display_uri", async (uri) => {
    log("display_uri received");
    let qrSvg = "";
    try {
      qrSvg = await QRCode.toString(uri, { type: "svg", margin: 2, width: 200, errorCorrectionLevel: "L" });
    } catch (err) {
      log(`qr generation failed: ${err.message}`);
    }
    emitEvent("display_uri", { uri, qrSvg });
  });
  provider.on("accountsChanged", (accounts) => {
    connectedAccounts = parseAccounts(accounts);
    emitEvent("accountsChanged", { accounts: connectedAccounts });
  });
  provider.on("chainChanged", (chainId) => {
    try {
      connectedChainIdHex = normalizeChainIdHex(chainId);
      emitEvent("chainChanged", { chainId: connectedChainIdHex });
    } catch (err) {
      log(`chainChanged handler error (non-fatal): ${err.message}`);
    }
  });
  provider.on("disconnect", () => {
    connectedAccounts = [];
    emitEvent("disconnect", {});
  });

  return provider;
}

async function connect(requiredChainId) {
  const wc = await ensureProvider(requiredChainId);
  const chainId = Number.isFinite(requiredChainId) ? Number(requiredChainId) : undefined;
  const connectChains = uniqueNumbers([chainId, 1, 11155111, 31337]);
  if (!wc.session) {
    log(`connecting session optionalChains=${connectChains.join(",")}`);
    const connectPromise = wc.connect({ optionalChains: connectChains });
    await Promise.race([
      connectPromise,
      new Promise((_, reject) =>
        setTimeout(() => reject(new Error(`connect timeout after ${connectTimeoutMs}ms`)), connectTimeoutMs)
      )
    ]);
  }
  log("session connected, requesting accounts/chain");
  const accountsRaw = await wc.request({ method: "eth_accounts", params: [] });
  const chainIdRaw = await wc.request({ method: "eth_chainId", params: [] });
  connectedAccounts = parseAccounts(accountsRaw);
  connectedChainIdHex = normalizeChainIdHex(chainIdRaw);
  return {
    accounts: connectedAccounts,
    chainId: connectedChainIdHex
  };
}

async function requestRpc(method, params) {
  if (!provider?.session) {
    throw new Error("WalletConnect session is not connected");
  }
  return await provider.request({ method, params });
}

async function handleCommand(msg) {
  const { id, method, params } = msg || {};
  if (typeof id !== "number") {
    throw new Error("Command is missing numeric id");
  }
  if (typeof method !== "string") {
    throw new Error("Command is missing method");
  }
  if (method === "ping") {
    return { id, result: { ok: true } };
  }
  if (method === "connect") {
    const chainId = params?.chainId;
    const required = typeof chainId === "string"
      ? Number.parseInt(chainId.startsWith("0x") ? chainId.slice(2) : chainId, chainId.startsWith("0x") ? 16 : 10)
      : typeof chainId === "number"
      ? chainId
      : undefined;
    log(`command connect chainId=${required ?? "none"}`);
    const result = await connect(required);
    return { id, result };
  }
  if (method === "request") {
    const rpcMethod = params?.method;
    if (typeof rpcMethod !== "string") {
      throw new Error("request.method missing");
    }
    const rpcParams = params?.params ?? [];
    const result = await requestRpc(rpcMethod, rpcParams);
    return { id, result };
  }
  if (method === "disconnect") {
    if (provider) {
      await provider.disconnect();
    }
    connectedAccounts = [];
    return { id, result: { ok: true } };
  }
  throw new Error(`Unknown helper method: ${method}`);
}

const rl = readline.createInterface({
  input: process.stdin,
  crlfDelay: Infinity
});

rl.on("line", async (line) => {
  const trimmed = line.trim();
  if (!trimmed) return;
  let msg;
  try {
    msg = JSON.parse(trimmed);
  } catch (error) {
    writeResponse({
      id: 0,
      error: {
        code: -32700,
        message: `Invalid JSON: ${String(error)}`
      }
    });
    return;
  }

  const id = typeof msg?.id === "number" ? msg.id : 0;
  try {
    const response = await handleCommand(msg);
    writeResponse(response);
  } catch (error) {
    log(`cmd error id=${id}: ${error instanceof Error ? error.message : String(error)}`);
    writeResponse({
      id,
      error: {
        code: -32000,
        message: error instanceof Error ? error.message : String(error)
      }
    });
  }
});

rl.on("close", () => {
  process.exit(0);
});
