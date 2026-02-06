# Client Devnet Launcher - Plan and Open Work

Date: 2026-02-04

## Decisions from user

- Keep client offline-only by default.
- In devnet mode, RPC access must go through a Rust proxy (no direct WebView RPC).
- Reuse `contracts/.devnet/devnet.json` by default.
- Cache downloaded bundles in `client/.vibefi/cache/<cid>`.
- Launcher UI should be bundled HTML in Wry now, with a clear path to move to React soon.
- Always show a list of dapps first; no auto-launch.
- CID verification is on by default.
- Always run `bun+vite`, unless build output is already cached.
- Do not expand signing beyond the existing offline signer behavior.
- Use verbose logging for now.

## Current state (baseline)

- `client` is a Wry app with an EIP-1193 shim and offline-only CSP.
- It can load a local bundle via `--bundle` and build it with `bun x vite build` into `.vibefi/dist`.
- `cli` already supports devnet, dapp list via logs, and IPFS bundle fetch/verify (used in e2e).
- `studio` only has a README at the moment.

## Target behavior

In devnet mode, the client should:

1) Read devnet config from `contracts/.devnet/devnet.json`.
2) Query on-chain logs for dapp registry events and list available dapps.
3) On selection, fetch bundle files from IPFS, verify CID, then build locally.
4) Load and run the built bundle inside Wry (still offline-only; RPC proxied by Rust).

## Implementation plan (client)

1) **Devnet mode + config**
   - Add CLI flags: `--devnet`, `--rpc`, `--ipfs-api`, `--ipfs-gateway`, `--cache-dir`.
   - Default `--devnet` to `contracts/.devnet/devnet.json` when present.

2) **Rust RPC proxy (required)**
   - Keep CSP `connect-src 'none'`.
   - Implement EIP-1193 request forwarding in Rust:
     - Forward read methods (`eth_chainId`, `net_version`, `eth_call`, `eth_getLogs`, etc.) to RPC.
     - Keep signing offline using existing signer (no additional signing features).

3) **Dapp list from logs**
   - Implement log queries against `dappRegistry` using Alloy/viem-equivalent in Rust.
   - Mirror CLI logic for DappPublished/Upgraded/Metadata/Paused/Unpaused/Deprecated.
   - Build a list of latest versions with status.

4) **IPFS fetch + CID verification**
   - Fetch `manifest.json` and all bundle files from gateway.
   - Verify CID using IPFS API `add --only-hash` and compare to root CID.

5) **Build + cache**
   - Cache bundles under `client/.vibefi/cache/<cid>`.
   - Build output under `<cache>/<cid>/.vibefi/dist`.
   - If `.vibefi/dist` exists and matches manifest, skip build.

6) **Launcher UI (HTML now, React later)**
   - Implement a minimal HTML UI in Wry that:
     - Lists dapps (name/version/status).
     - Allows selection and launch.
     - Shows progress/errors.
   - Design UI to be easily swapped for a React bundle later.

7) **Verbose logging**
   - Log all stages: devnet load, log fetch, IPFS fetch, verify, build, launch.

## MVP gaps / follow-up work

- **Launcher UX**: add progress/error states in the UI (right now it logs to a simple console box).
- **RPC proxy coverage**: validate the current allowlist against real dapps; expand read methods as needed.
- **Signing behavior**: currently keeps the offline demo signer and fake `eth_sendTransaction`; dapps needing real txs will fail.
- **Devnet config hardening**: validate required fields and surface errors in UI, not just stderr.
- **React launcher**: replace HTML UI with a minimal React build when ready.
- **Testing**: add a devnet e2e for client (start devnet + IPFS + launch selected dapp).
