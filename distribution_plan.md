# macOS Distribution Plan (cargo-packager)

## 1) Scope

This plan defines all required changes to distribute the Wry client on macOS using `cargo-packager`, producing:

- A signed `.app`
- A signed + notarized `.dmg`
- Separate Apple Silicon + Intel artifacts in phase 1 (`aarch64-apple-darwin`, `x86_64-apple-darwin`)

Linux/Windows packaging is intentionally out of scope for this document.

---

## 2) Current State Audit (what blocks shipping today)

> **All items below have been resolved.** See section 4 for details.

1. ~~No app/packaging metadata yet (identifier, icons, formats, resources) in `client/Cargo.toml`.~~ **Done.**
2. ~~UI build is coupled to Bun at compile time (`client/build.rs` runs `bun run build`), so packaging hosts must have Bun installed.~~ **Done** — `SKIP_UI_BUILD` env var added to `build.rs`.
3. ~~WalletConnect helper is resolved from source path via `env!("CARGO_MANIFEST_DIR")` in `client/src/walletconnect.rs`, which is invalid once installed as `.app`.~~ **Done** — new `runtime_paths.rs` module.
4. ~~WalletConnect runtime depends on `node` or `bun` being on `PATH`; packaged GUI apps should not rely on shell PATH.~~ **Done** — Bun vendored as external binary in app bundle.
5. ~~Default cache path is repo-relative (`client/.vibefi/cache`) in `client/src/config.rs`; packaged app should use per-user cache directory.~~ **Done** — uses `dirs::cache_dir()`.
6. ~~macOS app menu still references demo name (`"Wry EIP-1193 demo"`) in `client/src/main.rs`.~~ **Done** — renamed to `"VibeFi"`.

---

## 3) Target Packaging Architecture (macOS)

### 3.1 Packaging tool

Use `cargo-packager` (v0.11.8) with Cargo metadata config (`[package.metadata.packager]`) and macOS format targets:

- `app`
- `dmg`

### 3.2 Runtime dependency strategy for WalletConnect

Bundle both of these with the app:

1. A platform-matched Bun executable (vendored, per target triple via `external-binaries`).
2. A bundled WalletConnect helper script (single-file Bun bundle, 3.6 MB).

Runtime resolution order in app code (`src/runtime_paths.rs`):

1. Explicit env override (`VIBEFI_NODE_BIN`, `VIBEFI_WC_HELPER_SCRIPT`)
2. Bundled runtime/script from app bundle (`Contents/MacOS/bun`, `Contents/Resources/walletconnect-helper.mjs`)
3. PATH fallback (dev mode)

This guarantees production installs do not require global Node/Bun.

### 3.3 WalletConnect helper bundling

The `@walletconnect/ethereum-provider` npm package has CJS/ESM export mismatches in transitive
dependencies (`@reown/*`). Bun's bundler resolves this when forced to use ESM via `--conditions=module`:

```bash
bun build index.mjs --bundle --target=bun --conditions=module --outfile dist/walletconnect-helper.mjs
```

This produces a single 3.6 MB file (vs 395 MB `node_modules`) that runs with the vendored Bun binary.

---

## 4) Implemented Repo Changes

### 4.1 cargo-packager metadata in `client/Cargo.toml` ✅

Package renamed from `wry_eip1193_example` to `vibefi`. Full packager config added:

```toml
[package.metadata.packager]
product-name = "VibeFi"
identifier = "fi.vibefi.client"
formats = ["app", "dmg"]
icons = ["packaging/icons/vibefi.icns"]
out-dir = "target/packager"

resources = [
  "walletconnect-helper/dist/walletconnect-helper.mjs",
  "config/mainnet.json",
  "config/sepolia.json",
]

external-binaries = ["vendor/bun/bun"]

before-packaging-command = "bun install --cwd walletconnect-helper --frozen-lockfile && cd walletconnect-helper && bun run build:dist"

[package.metadata.packager.macos]
minimum-system-version = "12.0"
entitlements = "packaging/macos/entitlements.plist"

[package.metadata.packager.dmg]
background = "packaging/macos/dmg-background.png"
window-size = { width = 660, height = 420 }
app-position = { x = 180, y = 220 }
application-folder-position = { x = 480, y = 220 }
```

Notes:

- `hardened-runtime` is **not** a valid `cargo-packager` v0.11.8 key under `[macos]`; hardened runtime is applied automatically when a signing identity is present.
- DMG config lives under `[package.metadata.packager.dmg]`, not `[package.metadata.packager.macos.dmg]`.
- `cargo packager` does **not** build the binary itself — `cargo build --release` must run first (or be included in `before-packaging-command`). Currently excluded since the build is handled separately.

### 4.2 Packaging assets and structure ✅

Created:

- `client/packaging/icons/vibefi.icns` — placeholder icon (purple "V", generated from 1024x1024 PNG via `iconutil`)
- `client/packaging/icons/vibefi.png` — source PNG (1024x1024)
- `client/packaging/macos/entitlements.plist` — hardened runtime entitlements:
  - `com.apple.security.app-sandbox` = false
  - `com.apple.security.network.client` = true (outbound network for RPC/WC/IPFS)
  - `com.apple.security.cs.allow-unsigned-executable-memory` = true (for Bun JIT)
  - `com.apple.security.cs.allow-jit` = true (for Bun JIT)
- `client/packaging/macos/dmg-background.png` — placeholder DMG background (660x420, dark)
- `client/vendor/fetch-bun.sh` — downloads platform-matched Bun binaries from GitHub releases
- `client/vendor/bun/.gitkeep` — vendored binaries not checked in (downloaded via `fetch-bun.sh`)

Vendored Bun binaries (downloaded, gitignored):
- `client/vendor/bun/bun-aarch64-apple-darwin`
- `client/vendor/bun/bun-x86_64-apple-darwin`

### 4.3 Helper build output for packaging ✅

`client/walletconnect-helper/package.json` updated with:

```json
{
  "scripts": {
    "build:dist": "bun build index.mjs --bundle --target=bun --conditions=module --outfile dist/walletconnect-helper.mjs"
  }
}
```

Key detail: `--conditions=module` forces ESM resolution, working around the CJS/ESM mismatch in `@walletconnect/ethereum-provider` and its `@reown/*` transitive dependencies.

### 4.4 Runtime paths — app-bundle aware ✅

New module: `client/src/runtime_paths.rs`

- `macos_bundle_contents_dir()` — detects if running inside `.app` bundle by checking `Contents/MacOS/` layout
- `resolve_node_binary()` — env var → `Contents/MacOS/bun` → PATH probe (`bun`, `node`)
- `resolve_wc_helper_script()` — env var → `Contents/Resources/walletconnect-helper.mjs` → `CARGO_MANIFEST_DIR` dev fallback

`client/src/walletconnect.rs` updated to use `runtime_paths` module, removing the old inline `resolve_node_binary()` and hardcoded `CARGO_MANIFEST_DIR` path.

`client/src/main.rs` — added `mod runtime_paths`.

### 4.5 Per-user cache location ✅

`client/src/config.rs` — default cache path changed from `client/.vibefi/cache` to `dirs::cache_dir().join("VibeFi")` (resolves to `~/Library/Caches/VibeFi` on macOS). `dirs` crate v6 added to `Cargo.toml`. `cacheDir` config override still works as before.

### 4.6 Branding cleanup ✅

- `client/src/main.rs`: `menu::setup_macos_app_menu("VibeFi")`
- Package renamed to `vibefi` in `Cargo.toml`

### 4.7 build.rs guard ✅

`client/build.rs` — skips `bun run build` when `SKIP_UI_BUILD` env var is set. Also refactored to use shared `run_bun_step()` / `run_with_console_handling()` helpers, and now runs `bun install` as a separate step before the build.

### 4.8 .gitignore updates ✅

Added to `client/.gitignore`:
- `vendor/bun/bun-*` (vendored binaries, ~50MB each)
- `walletconnect-helper/dist/` (build output)

---

## 5) Signing + Notarization Plan (manual + CI)

> **Status: Not yet implemented.** This is the next phase.

### 5.1 One-time Apple setup (manual)

1. Apple Developer Program membership active.
2. Create/download **Developer ID Application** certificate.
3. Install cert in macOS keychain for local signing (or export for CI).
4. Set up notarization auth (choose one):
   - App Store Connect API key (recommended), or
   - Apple ID + app-specific password + Team ID.

### 5.2 Secret management

Keep secrets out of git and config files.

Use environment variables in local shell / CI secrets:

- Signing:
  - `APPLE_SIGNING_IDENTITY`
  - `APPLE_CERTIFICATE`
  - `APPLE_CERTIFICATE_PASSWORD`
- Notarization:
  - `APPLE_API_KEY`
  - `APPLE_API_ISSUER`
  - `APPLE_API_KEY_PATH`
  - or `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`

(Use the exact set supported by your `cargo-packager`/tooling version.)

### 5.3 Packaging commands

Install:

```bash
cargo install cargo-packager --locked
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

Build + package per architecture:

```bash
cd client
cargo build --release
cargo packager --release --formats app
```

Or for cross-architecture:

```bash
cargo build --release --target aarch64-apple-darwin
cargo packager --release --target-triple aarch64-apple-darwin

cargo build --release --target x86_64-apple-darwin
cargo packager --release --target-triple x86_64-apple-darwin
```

Optional universal build later:

```bash
cargo packager --release --target-triple universal-apple-darwin
```

### 5.4 Post-build verification (must pass)

For each generated `.app` and `.dmg`:

```bash
codesign --verify --deep --strict --verbose=2 /path/to/VibeFi.app
spctl --assess --type execute --verbose /path/to/VibeFi.app
xcrun stapler validate /path/to/VibeFi.app
hdiutil verify /path/to/VibeFi.dmg
spctl --assess --type open --verbose /path/to/VibeFi.dmg
```

If app opens with quarantine warning on clean machine, notarization/stapling is incomplete.

---

## 6) CI Workflow Spec (macOS only)

> **Status: Not yet implemented.**

Add a macOS release workflow (GitHub Actions or your CI):

1. Checkout repo.
2. Install Rust stable (≥1.85 for edition 2024) and macOS targets.
3. Install Bun (build-time requirement for `build.rs` and helper bundling).
4. Install `cargo-packager`.
5. Run `vendor/fetch-bun.sh` to download Bun binaries for target architecture.
6. Import signing certificate/keychain in CI.
7. Provide notarization secrets as env vars.
8. Run `cargo build --release --target <target>`.
9. Run `cargo packager --release --target-triple <target>`.
10. Run signature/notarization verification commands.
11. Upload `.app`/`.dmg` artifacts.

Run two jobs in parallel:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`

---

## 7) Acceptance Criteria

A build is "distribution-ready" when all are true:

1. ✅ `cargo packager` produces `.app` for the current mac target (67 MB, unsigned).
2. ✅ App launches from packaged `.app` bundle.
3. ✅ WalletConnect helper bundled as single 3.6 MB file (no `node_modules` shipped).
4. ⬜ `codesign`, `spctl`, `stapler`, and `hdiutil` verification commands pass (requires signing setup).
5. ✅ No repo-relative writes occur at runtime by default (cache path uses `~/Library/Caches/VibeFi`).
6. ✅ Product branding is consistent (`VibeFi`) in app menu/title/package metadata.
7. ⬜ `.dmg` produced and verified (requires signing for notarization).
8. ⬜ Both `aarch64-apple-darwin` and `x86_64-apple-darwin` builds verified.

---

## 8) Implementation Progress

| # | Step | Status |
|---|------|--------|
| 1 | Add packager metadata + asset folders | ✅ Done |
| 2 | Bundle Bun binaries and helper dist build | ✅ Done |
| 3 | Implement runtime path resolution changes | ✅ Done |
| 4 | Move default cache path to per-user location | ✅ Done |
| 5 | Ship unsigned local `.app` and smoke-test | ✅ Done (67 MB, launches successfully) |
| 6 | Replace placeholder icon with real branding | ⬜ Pending |
| 7 | Add signing secrets and notarization | ⬜ Pending (requires Apple Developer setup) |
| 8 | Enable CI release artifacts | ⬜ Pending |

---

## 9) References

- cargo-packager docs and config:
  - https://docs.rs/crate/cargo-packager/latest
  - https://docs.crabnebula.dev/packager/configuration/
- Tauri packaging CLI (`cargo-packager`) usage:
  - https://v2.tauri.app/distribute/packaging-cli/
- macOS app bundle behavior (paths/PATH caveats):
  - https://v2.tauri.app/distribute/macos-application-bundle/
- Tauri macOS signing/notarization environment setup:
  - https://v2.tauri.app/distribute/sign/macos/
- Apple notarization process:
  - https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution
  - https://developer.apple.com/documentation/security/customizing-the-notarization-workflow
