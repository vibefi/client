# VibeFi Client

Desktop application (Wry/Rust) that fetches, builds, and runs approved dapps from the VibeFi on-chain registry.

- Embeds a **WebView** with an injected **`window.ethereum`** provider (EIP-1193)
- Bridges `ethereum.request(...)` to a Rust backend via Wry IPC
- Uses **Alloy** for local dev-key signing (default)
- Supports **WalletConnect v2** via a local helper process (`walletconnect-helper/`)
- **Blocks outbound network**: only loads `app://...` assets and sets `connect-src 'none'`

## Usage

```bash
cargo run                                                     # no config, home screen only
cargo run -- --config config/sepolia.json                     # connect to Sepolia testnet
cargo run -- --config ../contracts/.devnet/devnet.json        # local devnet
cargo run -- --config config/sepolia.json --bundle ../dapp-examples/counter  # bundle a local dapp
cargo run -- --help                                           # show all CLI flags
```

| Flag | Description |
|------|-------------|
| `--config <PATH>` | Path to a network config JSON file (e.g. `config/sepolia.json`) |
| `--bundle <PATH>` | Path to a local dapp project directory to bundle and serve |
| `--no-build` | Skip the `bun build` step when using `--bundle` |

If `--config` is omitted, the client looks for a default config via `runtime_paths::resolve_default_config()`.

## Configuration

The client resolves configuration from multiple layers. Later layers override earlier ones. Everything is merged into a single `ResolvedConfig` struct at startup.

### Layer 1 — Deployment JSON (`AppConfig`)

A JSON file passed via `--config`. This is the primary source for network and contract settings. Fields use camelCase to match deployment tooling output.

```jsonc
{
  "chainId": 11155111,                // required, must be > 0
  "rpcUrl": "https://...",            // default: "http://127.0.0.1:8546"
  "dappRegistry": "0xFb84...",        // hex address of the DappRegistry contract
  "deployBlock": 10239268,            // starting block for event log queries
  "localNetwork": false,              // true for local devnets (enables demo wallet key)
  "developerPrivateKey": null,        // optional private key for local signing
  "ipfsApi": null,                    // IPFS API endpoint (default: "http://127.0.0.1:5001")
  "ipfsGateway": null,                // IPFS gateway endpoint (default: "http://127.0.0.1:8080")
  "ipfsFetchBackend": "helia",        // "helia" (verified fetch) or "localnode"
  "ipfsHeliaGateways": [...],         // list of Helia trustless gateways
  "ipfsHeliaRouters": [...],          // list of Helia DHT routers
  "ipfsHeliaTimeoutMs": 15000,        // Helia fetch timeout in milliseconds
  "cacheDir": null,                   // bundle cache directory (default: OS cache dir / VibeFi)
  "walletConnect": {                  // optional WalletConnect settings
    "projectId": "...",
    "relayUrl": "..."
  }
}
```

Extra fields (e.g. `deployer`, `vfiGovernor`) are silently ignored, so deployment output can be used as-is.

Validation runs at load time: `chainId` must not be 0, `dappRegistry` (if non-empty) must be valid hex, and `rpcUrl` must use an `http://`, `https://`, `ws://`, or `wss://` scheme.

### Layer 2 — Environment variables (`VIBEFI_*`)

Environment variables override or fill specific values from the deployment JSON.

| Variable | Overrides | Type |
|----------|-----------|------|
| `VIBEFI_RPC_URL` | `rpcUrl` | URL string |
| `VIBEFI_WC_PROJECT_ID` | `walletConnect.projectId` (when config value is missing) | string |
| `VIBEFI_WC_RELAY_URL` | `walletConnect.relayUrl` | string |
| `VIBEFI_ENABLE_DEVTOOLS` | WebView devtools (release builds) | bool (`1`/`true`/`yes`/`on`) |

In debug builds (`cfg!(debug_assertions)`), devtools are always enabled regardless of the env var.

### Layer 3 — Compile-time flags

| Flag | Effect |
|------|--------|
| `cfg!(debug_assertions)` | Enables devtools, selects `Dev` log profile |
| `option_env!("VIBEFI_EMBEDDED_WC_PROJECT_ID")` | Fallback WalletConnect project ID embedded into release binaries at build time |

### Layer 4 — User settings (runtime, not in `ResolvedConfig`)

A `settings.json` file stored alongside the config file (e.g. `config/settings.json`). These are **not** baked into `ResolvedConfig` because they can change at runtime through the Settings panel.

```jsonc
{
  "rpcEndpoints": [                   // ordered list with failover
    { "url": "https://...", "label": "Primary" },
    { "url": "https://...", "label": "Fallback" }
  ],
  "ipfs": {
    "fetchBackend": "helia",          // overrides config ipfsFetchBackend
    "gatewayEndpoint": "https://..."  // overrides config ipfsGateway
  }
}
```

User settings are merged at the point of use (e.g. launching a dapp, reading IPFS settings), not at startup.

### Resolution flow

```
CLI args (--config, --bundle, --no-build)
  |
  v
Deployment JSON (AppConfig)          -- deserialized, validated
  |
  v
Environment variables (VIBEFI_*)     -- override specific fields
  |
  v
Compile-time flags                   -- debug_assertions -> devtools
  |
  v
ResolvedConfig                       -- built once, stored as Arc<ResolvedConfig> in AppState
  :
  : (at call sites, not startup)
  v
User settings (settings.json)       -- runtime-mutable via Settings panel
```

## Logging

Logging initializes **before** config loading and resolves its own env vars independently.

| Variable | Description |
|----------|-------------|
| `RUST_LOG` | Standard tracing filter (highest priority) |
| `VIBEFI_LOG` | VibeFi-specific filter (if `RUST_LOG` is unset) |
| `VIBEFI_LOG_PROFILE` | `dev`, `user`, or `all` — selects a preset filter |
| `VIBEFI_LOG_DIR` | Override the log file directory |

Default profiles:

| Profile | When | Filter |
|---------|------|--------|
| Dev | Debug builds or `CARGO` env present | `off,vibefi=trace,vibefi::helper=debug` |
| User | Release builds | `info` |
| All | `VIBEFI_LOG_PROFILE=all` | `trace` |

Log output goes to stderr and rolling daily files:
- Linux: `~/.local/share/VibeFi/logs`
- macOS: `~/Library/Application Support/VibeFi/logs`
- Windows: `%LOCALAPPDATA%\VibeFi\logs`

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

## WalletConnect

Install helper dependencies once:

```bash
cd walletconnect-helper && bun install && cd ..
```

WalletConnect is configured via `walletConnect.projectId` in the config JSON, or the `VIBEFI_WC_PROJECT_ID` env var:

```bash
VIBEFI_WC_PROJECT_ID=your_project_id cargo run -- --config config/sepolia.json
```

Optional relay override:

```bash
VIBEFI_WC_PROJECT_ID=your_project_id \
VIBEFI_WC_RELAY_URL=wss://your-relay.example \
cargo run -- --config config/sepolia.json
```

For packaged installers, embed a compile-time fallback directly into the binary (without writing to config/settings files):

```bash
VIBEFI_EMBED_WC_PROJECT_ID=your_project_id cargo build --release
VIBEFI_EMBED_WC_PROJECT_ID=your_project_id cargo packager --release
```

`VIBEFI_EMBED_WC_PROJECT_ID` is consumed by `build.rs` and baked into the binary as a fallback only. Runtime `VIBEFI_WC_PROJECT_ID` still works and takes precedence over the embedded value.

## Linux build deps (Ubuntu/Debian)

```bash
sudo apt-get update
sudo apt-get install -y \
  pkg-config \
  libgtk-3-dev \
  libgdk-pixbuf-2.0-dev \
  libglib2.0-dev \
  libgobject-2.0-dev \
  libwebkit2gtk-4.0-dev
```

## Bundled dapps

Run a bundled dapp (expects `manifest.json` in the bundle directory):

```bash
cargo run -- --bundle /path/to/bundle
```

The bundle build step uses `bun` and `vite` from the bundle's `package.json`.
You can produce bundles with the CLI `package` command.

## IPFS retrieval

Dapp bundles are fetched from IPFS using one of two backends, configurable in Settings:

- **Helia Verified Fetch** (default): Fetches content via trustless HTTP gateways. Every block is cryptographically verified locally against the Merkle DAG structure — the CID you request is the CID you get. No local IPFS node required.
- **Local IPFS Node**: For advanced users running their own IPFS daemon (e.g. Kubo). Fetches from `http://127.0.0.1:8080` by default. The local node is implicitly trusted since you control it.

Helia is the recommended default because it provides strong integrity guarantees without requiring any local infrastructure.
Helia fetches also automatically retry up to 3 total attempts with short backoff for transient network failures.

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

- WalletConnect project id can be embedded from GitHub secret `VIBEFI_WC_PROJECT_ID` (wired to build env `VIBEFI_EMBED_WC_PROJECT_ID`)

**macOS:**
- `.dmg` installer (Apple Silicon and Intel)
- `.app` bundle archives (Apple Silicon and Intel)

**Linux (x86_64):**
- `.deb` package
- `.AppImage` portable executable

**Windows (x86_64):**
- `.msi` installer

All artifacts are attached to the GitHub release and available for download.
