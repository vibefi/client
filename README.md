# Wry + EIP-1193 injected provider

This is a minimal example of:

- A **Wry** desktop app that embeds a WebView
- Injects a **`window.ethereum`** provider shim (EIP-1193-style)
- Bridges `ethereum.request(...)` to a Rust backend via Wry IPC
- Uses **Alloy** for local dev-key signing (default)
- Supports **WalletConnect v2** via a local helper process (`walletconnect-helper/`)
- **Blocks outbound network**: only loads `app://...` assets and sets `connect-src 'none'`.

## Run (local signer)

```bash
cargo run
```

## Run (devnet)

This assumes you've got the devnet running.

```bash
cargo run -- --config ../contracts/.devnet/devnet.json
```

## Logging

- Logging uses `tracing` with stderr + rolling file output.
- Default log file directory:
  - Linux: `~/.local/share/VibeFi/logs`
  - macOS: `~/Library/Application Support/VibeFi/logs`
  - Windows: `%LOCALAPPDATA%\\VibeFi\\logs`
- Log profile defaults:
  - `cargo run`: verbose for VibeFi-only targets (`vibefi=trace`)
  - packaged/user app runs: quieter (`info`)
- Built-in profiles via `VIBEFI_LOG_PROFILE`:
  - `dev`: verbose VibeFi logs only (suppresses noisy dependency logs)
  - `user`: quieter defaults for end users
  - `all`: everything (`trace`) including dependency crates (`reqwest`, `hyper`, etc.)
- Overrides:
  - `RUST_LOG` (highest priority)
  - `VIBEFI_LOG`
  - `VIBEFI_LOG_PROFILE=dev|user|all`
  - `VIBEFI_LOG_DIR=/custom/path`

Show everything example:

```bash
VIBEFI_LOG_PROFILE=all cargo run -- --config ../contracts/.devnet/devnet.json
```


## Internal UI (React)

Built-in UI pages and preload scripts are bundled from `internal-ui/src` to
`internal-ui/dist/*` and loaded over `app://...`.

`cargo build` / `cargo run` automatically run the internal-ui build via `build.rs`,
so `bun` must be installed.

Manual rebuild (optional):

```bash
cd internal-ui
bun install
bun run build
```

## Run with WalletConnect

Install helper dependencies once:

```bash
cd walletconnect-helper
bun install
cd ..
```

Run the client with WalletConnect enabled:

```bash
VIBEFI_WC_PROJECT_ID=your_project_id cargo run -- --wallet walletconnect
```

Optional relay override:

```bash
VIBEFI_WC_PROJECT_ID=your_project_id \
VIBEFI_WC_RELAY_URL=wss://your-relay.example \
cargo run -- --wallet walletconnect
```

Linux build deps (Ubuntu/Debian):

```bash
#!/usr/bin/env bash
set -euo pipefail

sudo apt-get update
sudo apt-get install -y \
  pkg-config \
  libgtk-3-dev \
  libgdk-pixbuf-2.0-dev \
  libglib2.0-dev \
  libgobject-2.0-dev \
  libwebkit2gtk-4.0-dev

```

Run a bundled dapp (expects `manifest.json` in the bundle directory):

```bash
cargo run -- --bundle /path/to/bundle
```

Run a bundled dapp with WalletConnect (for example, a bundle produced from `dapp-examples/uniswap-v2`):

```bash
VIBEFI_WC_PROJECT_ID=your_project_id \
cargo run -- --bundle /path/to/packaged-bundle --wallet walletconnect
```

You can produce `/path/to/packaged-bundle` with the CLI `package` command.

The bundle build step uses `bun` and `vite` from the bundle's `package.json`.

## IPFS retrieval

Dapp bundles are fetched from IPFS using one of two backends, configurable in Settings:

- **Helia Verified Fetch** (default): Fetches content via trustless HTTP gateways. Every block is cryptographically verified locally against the Merkle DAG structure — the CID you request is the CID you get. No local IPFS node required.
- **Local IPFS Node**: For advanced users running their own IPFS daemon (e.g. Kubo). Fetches from `http://127.0.0.1:8080` by default. The local node is implicitly trusted since you control it.

Helia is the recommended default because it provides strong integrity guarantees without requiring any local infrastructure.

## What is sandboxed?

- The WebView only allows navigation to `app://...` and `about:blank`.
- The content is served via Wry's `with_custom_protocol` from embedded assets.
- CSP includes `connect-src 'none'` to prevent `fetch`/XHR/WebSockets.

## Wallet backends

- `local` (default): hard-coded/demo private key fallback for local testing only.
- `walletconnect`: remote signer via WalletConnect; `eth_requestAccounts` triggers pairing and logs a `wc:` URI.

## Releases

Release packages are automatically built and published when a version tag is pushed:

```bash
git tag v0.1.0
git push origin v0.1.0
```

This triggers the release workflow which builds and uploads:

**macOS:**
- `.dmg` installer (Apple Silicon and Intel)
- `.app` bundle archives (Apple Silicon and Intel)

**Linux (x86_64):**
- `.deb` package
- `.AppImage` portable executable

**Windows (x86_64):**
- `.msi` installer

All artifacts are attached to the GitHub release and available for download.
