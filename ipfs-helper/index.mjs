#!/usr/bin/env node

import process from "node:process";
import readline from "node:readline";
import { createHeliaHTTP } from "@helia/http";
import { trustlessGateway } from "@helia/block-brokers";
import { delegatedHTTPRouting, httpGatewayRouting } from "@helia/routers";
import { unixfs } from "@helia/unixfs";
import { CID } from "multiformats/cid";

function log(message) {
  process.stderr.write(`[ipfs-helper] ${message}\n`);
}

function writeResponse(payload) {
  process.stdout.write(`${JSON.stringify(payload)}\n`);
}

function parseJsonList(raw, label) {
  if (!raw || typeof raw !== "string") return [];
  try {
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((entry) => typeof entry === "string" && entry.trim().length > 0);
  } catch (error) {
    log(`failed to parse ${label}: ${String(error)}`);
    return [];
  }
}

const heliaGateways = parseJsonList(
  process.env.VIBEFI_IPFS_HELIA_GATEWAYS || "",
  "gateways"
);
const heliaRouters = parseJsonList(
  process.env.VIBEFI_IPFS_HELIA_ROUTERS || "",
  "routers"
);

const DEFAULT_GATEWAYS = [
  "https://trustless-gateway.link",
  "https://cloudflare-ipfs.com",
  "https://ipfs.filebase.io",
  "https://ipfs.io",
  "https://dweb.link"
];
const DEFAULT_ROUTERS = [
  "https://delegated-ipfs.dev",
  "https://cid.contact",
  "https://indexer.pinata.cloud"
];

let heliaPromise = null;

async function getHelia() {
  if (!heliaPromise) {
    const gateways = heliaGateways.length > 0 ? heliaGateways : DEFAULT_GATEWAYS;
    const routers = heliaRouters.length > 0 ? heliaRouters : DEFAULT_ROUTERS;

    log(`creating HTTP-only Helia node (no libp2p)`);
    log(`gateways: ${JSON.stringify(gateways)}`);
    log(`routers: ${JSON.stringify(routers)}`);

    heliaPromise = (async () => {
      const helia = await createHeliaHTTP({
        blockBrokers: [
          trustlessGateway(),
        ],
        routers: [
          ...routers.map((r) => delegatedHTTPRouting(r)),
          httpGatewayRouting({ gateways }),
        ],
      });
      log("Helia HTTP node created");
      return { helia, fs: unixfs(helia) };
    })();
  }
  return await heliaPromise;
}

/**
 * Parse an ipfs:// URL into { cid, path }.
 * e.g. "ipfs://bafyXYZ/manifest.json" → { cid: CID, path: "manifest.json" }
 */
function parseIpfsUrl(url) {
  if (typeof url !== "string" || !url.startsWith("ipfs://")) {
    throw new Error("fetch.url must be an ipfs:// URL");
  }
  const withoutScheme = url.slice("ipfs://".length);
  const slashIdx = withoutScheme.indexOf("/");
  const cidStr = slashIdx === -1 ? withoutScheme : withoutScheme.slice(0, slashIdx);
  const path = slashIdx === -1 ? "" : withoutScheme.slice(slashIdx + 1);
  return { cid: CID.parse(cidStr), path };
}

/**
 * Race a promise against a hard timeout. Returns the promise result or throws
 * a timeout error. Unlike AbortSignal.timeout(), this guarantees the caller
 * gets unblocked even if the underlying async iterator ignores the signal.
 */
function withHardTimeout(promise, ms, label) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(new Error(`timeout after ${ms}ms: ${label}`));
    }, ms);
    promise.then(
      (v) => { clearTimeout(timer); resolve(v); },
      (e) => { clearTimeout(timer); reject(e); },
    );
  });
}

async function fetchIpfsInner(url, timeoutMs) {
  const { cid, path } = parseIpfsUrl(url);
  const { fs } = await getHelia();

  const timeout = Number.isFinite(timeoutMs) && timeoutMs > 0
    ? Number(timeoutMs)
    : 30_000;

  const controller = new AbortController();
  const { signal } = controller;

  // Hard timeout wrapper kills the operation even if Helia ignores the signal.
  const hardTimer = setTimeout(() => {
    log(`hard timeout (${timeout}ms) reached for ${url}`);
    controller.abort();
  }, timeout);

  log(`fetch start: ${url} (timeout=${timeout}ms)`);
  const t0 = Date.now();

  try {
    const chunks = [];
    let chunkCount = 0;
    for await (const chunk of fs.cat(cid, { path: path || undefined, signal })) {
      chunks.push(chunk);
      chunkCount++;
      if (chunkCount === 1) {
        log(`fetch first chunk after ${Date.now() - t0}ms (${chunk.length} bytes)`);
      }
    }
    const body = Buffer.concat(chunks);
    log(`fetch done: ${body.length} bytes in ${chunkCount} chunks (${Date.now() - t0}ms)`);

    return {
      status: 200,
      headers: {
        "content-length": String(body.length),
        "x-ipfs-path": `/ipfs/${cid.toString()}${path ? `/${path}` : ""}`,
      },
      bodyBase64: body.toString("base64"),
    };
  } finally {
    clearTimeout(hardTimer);
  }
}

async function fetchIpfs(url, timeoutMs) {
  const timeout = Number.isFinite(timeoutMs) && timeoutMs > 0
    ? Number(timeoutMs)
    : 30_000;

  // Belt-and-suspenders: wrap the entire fetch in a hard timeout promise race
  // so we always respond even if the Helia internals swallow the abort.
  return withHardTimeout(
    fetchIpfsInner(url, timeoutMs),
    timeout + 5_000, // 5s grace beyond the inner timeout
    url,
  );
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
  if (method === "fetch") {
    log(`cmd fetch id=${id} url=${params?.url} timeout=${params?.timeoutMs}`);
    const result = await fetchIpfs(params?.url, params?.timeoutMs);
    return { id, result };
  }
  throw new Error(`Unknown helper method: ${method}`);
}

const rl = readline.createInterface({
  input: process.stdin,
  crlfDelay: Infinity,
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
        message: `Invalid JSON: ${String(error)}`,
      },
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
        message: error instanceof Error ? error.message : String(error),
      },
    });
  }
});
