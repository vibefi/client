# macOS Distribution Plan (cargo-packager)

## 1) Scope

This plan defines all required changes to distribute the Wry client on macOS using `cargo-packager`, producing:

- A signed `.app`
- A signed + notarized `.dmg`
- Separate Apple Silicon + Intel artifacts in phase 1 (`aarch64-apple-darwin`, `x86_64-apple-darwin`)

Linux/Windows packaging is intentionally out of scope for this document.

---

## 2) Current State Audit (what blocks shipping today)

From the current `client` code:

1. No app/packaging metadata yet (identifier, icons, formats, resources) in `client/Cargo.toml`.
2. UI build is coupled to Bun at compile time (`client/build.rs` runs `bun run build`), so packaging hosts must have Bun installed.
3. WalletConnect helper is resolved from source path via `env!("CARGO_MANIFEST_DIR")` in `client/src/walletconnect.rs`, which is invalid once installed as `.app`.
4. WalletConnect runtime depends on `node` or `bun` being on `PATH`; packaged GUI apps should not rely on shell PATH.
5. Default cache path is repo-relative (`client/.vibefi/cache`) in `client/src/config.rs`; packaged app should use per-user cache directory.
6. macOS app menu still references demo name (`"Wry EIP-1193 demo"`) in `client/src/main.rs`.

---

## 3) Target Packaging Architecture (macOS)

### 3.1 Packaging tool

Use `cargo-packager` with Cargo metadata config (`[package.metadata.packager]`) and macOS format targets:

- `app`
- `dmg`

### 3.2 Runtime dependency strategy for WalletConnect

Bundle both of these with the app:

1. A platform-matched Bun executable (vendored, per target triple).
2. A bundled WalletConnect helper script artifact (single-file bundle).

Runtime resolution order in app code should become:

1. Explicit env override (`VIBEFI_NODE_BIN`, optional `VIBEFI_WC_HELPER_SCRIPT`)
2. Bundled runtime/script from app bundle
3. PATH fallback (dev mode)

This guarantees production installs do not require global Node/Bun.

---

## 4) Required Repo Changes

### 4.1 Add cargo-packager metadata in `client/Cargo.toml`

Add package-level metadata and macOS config:

```toml
[package.metadata.packager]
product-name = "VibeFi"
identifier = "fi.vibefi.client"
formats = ["app", "dmg"]
icons = ["packaging/icons/vibefi.icns"]
out-dir = "target/packager"

# Includes helper JS artifact and any runtime config files needed by app startup.
resources = [
  "walletconnect-helper/dist/walletconnect-helper.mjs",
  "config/mainnet.json",
  "config/sepolia.json",
]

# Expects files:
# - vendor/bun/bun-aarch64-apple-darwin
# - vendor/bun/bun-x86_64-apple-darwin
external-binaries = ["vendor/bun/bun"]

before-packaging-command = """
bun --cwd walletconnect-helper install --frozen-lockfile
bun --cwd walletconnect-helper run build:dist
cargo build --release
"""

[package.metadata.packager.macos]
minimum-system-version = "12.0"
hardened-runtime = true
entitlements = "packaging/macos/entitlements.plist"

[package.metadata.packager.macos.dmg]
background = "packaging/macos/dmg-background.png"
window-size = { width = 660, height = 420 }
application-position = { x = 180, y = 220 }
app-folder-position = { x = 480, y = 220 }
```

Notes:

- Keep values that cannot live in config files (notarization credentials, signing cert material) in environment variables/secrets.
- Confirm exact key casing with your installed `cargo-packager` version if parser errors appear.

### 4.2 Add packaging assets and structure

Create:

- `client/packaging/icons/vibefi.icns`
- `client/packaging/icons/vibefi.png` (source icon, 1024x1024)
- `client/packaging/macos/entitlements.plist`
- `client/packaging/macos/dmg-background.png`
- `client/vendor/bun/bun-aarch64-apple-darwin`
- `client/vendor/bun/bun-x86_64-apple-darwin`

Set execute bit on vendored Bun binaries:

```bash
chmod +x client/vendor/bun/bun-aarch64-apple-darwin
chmod +x client/vendor/bun/bun-x86_64-apple-darwin
```

### 4.3 Add helper build output for packaging

Update `client/walletconnect-helper/package.json`:

- Add script:

```json
{
  "scripts": {
    "build:dist": "bun build index.mjs --bundle --target=node --outfile dist/walletconnect-helper.mjs"
  }
}
```

Rationale:

- Produces one deployable helper file without shipping full `node_modules`.
- Works with bundled Bun runtime.

### 4.4 Make runtime paths app-bundle aware

### Files to change

- `client/src/walletconnect.rs`
- (recommended new helper module) `client/src/runtime_paths.rs`
- `client/src/main.rs` (module import)

### Required behavior

1. Replace compile-time source path assumption for helper script.
2. Resolve script from app bundle first in production.
3. Resolve bundled Bun executable path in packaged app.

### Proposed resolution logic

For helper script:

1. `VIBEFI_WC_HELPER_SCRIPT` if set.
2. `Contents/Resources/walletconnect-helper/dist/walletconnect-helper.mjs` (packaged mac app).
3. `client/walletconnect-helper/index.mjs` via `CARGO_MANIFEST_DIR` (dev fallback).

For runtime executable:

1. `VIBEFI_NODE_BIN` if set.
2. `Contents/MacOS/bun` (external binary in app bundle).
3. PATH probe (`node`, then `bun`) for local development.

### 4.5 Use per-user cache location by default

Change default cache path in `client/src/config.rs`:

- From repo-relative `client/.vibefi/cache`
- To user cache dir, e.g.:
  - `~/Library/Caches/VibeFi/cache` on macOS

Implementation detail:

- Add `dirs` crate (or equivalent) and compute OS-standard cache dir.
- Keep `cacheDir` config override behavior unchanged.

### 4.6 Branding cleanup for production build

Update app-facing names:

- `client/src/main.rs`: change `menu::setup_macos_app_menu("Wry EIP-1193 demo")` to `"VibeFi"`.
- Ensure package/binary naming aligns with desired product name.

---

## 5) Signing + Notarization Plan (manual + CI)

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

Build/package per architecture:

```bash
cd client
cargo packager --release --target-triple aarch64-apple-darwin
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

Add a macOS release workflow (GitHub Actions or your CI):

1. Checkout repo.
2. Install Rust stable and macOS targets.
3. Install Bun (build-time requirement for `build.rs` and helper bundling).
4. Install `cargo-packager`.
5. Import signing certificate/keychain in CI.
6. Provide notarization secrets as env vars.
7. Run `cargo packager --release --target-triple <target>`.
8. Run signature/notarization verification commands.
9. Upload `.app`/`.dmg` artifacts.

Run two jobs in parallel:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`

---

## 7) Acceptance Criteria

A build is “distribution-ready” when all are true:

1. `cargo packager` produces `.app` and `.dmg` for both mac targets.
2. App launches from `/Applications` on a clean macOS machine.
3. WalletConnect works without globally installed Node/Bun.
4. `codesign`, `spctl`, `stapler`, and `hdiutil` verification commands pass.
5. No repo-relative writes occur at runtime by default (cache path uses user cache dir).
6. Product branding is consistent (`VibeFi`) in app menu/title/package metadata.

---

## 8) Suggested Implementation Order

1. Add packager metadata + asset folders.
2. Bundle Bun binaries and helper dist build.
3. Implement runtime path resolution changes.
4. Move default cache path to per-user location.
5. Ship unsigned local `.app` first and smoke-test.
6. Add signing secrets and notarization.
7. Enable CI release artifacts.

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
