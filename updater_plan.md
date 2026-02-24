# VibeFi Client Onchain-Updater Plan (EIP-1271 Safe Signature)

## Goal
Implement secure auto-updating for the Rust desktop `client` where update authorization comes from an Ethereum **Safe multisig** via **EIP-1271** checks. The updater must only apply releases whose manifest is signed by the Safe-approved signer set.

## Security Model
- Trust anchors are **baked into the binary** as a chain->Safe map (at minimum mainnet + Sepolia).
- Update manifest is signed off-chain using EIP-712 typed data.
- Client verifies authorization by calling `isValidSignature(bytes32,bytes)` on the chain-specific Safe via the app's active RPC URL flow (`settings.json` overrides -> resolved RPC default).
- Release artifact must also pass local SHA-256 verification before install.
- Optional but recommended: keep platform code-signing/notarization checks as a second independent trust layer.

## Architecture Overview
1. App checks for updates (startup + interval + manual trigger).
2. App downloads manifest JSON from update endpoint.
3. App computes EIP-712 digest for manifest payload.
4. App calls chain-specific Safe `isValidSignature(digest, signature)`.
5. If valid and version is newer, download platform artifact.
6. Verify SHA-256 equals manifest hash.
7. Handoff to OS-specific installer helper and relaunch.

## Manifest + Signing Spec
Create a new versioned manifest format (`manifestVersion = 1`):

```json
{
  "manifestVersion": 1,
  "channel": "stable",
  "version": "0.3.0",
  "minSupportedVersion": "0.2.0",
  "releaseTime": "2026-02-24T00:00:00Z",
  "chainId": 11155111,
  "safeAddress": "0x...",
  "artifacts": {
    "macos-aarch64": { "url": "https://...", "sha256": "...", "size": 12345 },
    "macos-x64": { "url": "https://...", "sha256": "...", "size": 12345 },
    "windows-x64": { "url": "https://...", "sha256": "...", "size": 12345 },
    "linux-x64-appimage": { "url": "https://...", "sha256": "...", "size": 12345 },
    "linux-x64-deb": { "url": "https://...", "sha256": "...", "size": 12345 }
  },
  "notes": "...",
  "eip712": {
    "domain": {
      "name": "VibeFiUpdater",
      "version": "1",
      "chainId": 11155111,
      "verifyingContract": "0xSAFE"
    },
    "signature": "0x..."
  }
}
```

Two manifest artifacts are produced:
- `manifest.unsigned.json`: canonical payload without signature, plus computed digest metadata.
- `manifest.json`: final publishable manifest with the Safe signature embedded.

Typed data primary type: `UpdateManifest` with canonical field order:
- `string channel`
- `string version`
- `string minSupportedVersion`
- `string releaseTime`
- `bytes32 artifactsRoot`
- `bytes32 notesHash`

Where:
- `artifactsRoot` = keccak256 Merkle root over normalized `artifacts` entries.
- `notesHash` = keccak256(utf8(notes)).

This avoids signing large dynamic JSON directly and makes digest deterministic.

## Concrete Code Changes

## 1) Config wiring
### Files
- `client/src/config/app_config.rs`
- `client/src/config/resolved.rs`
- `client/src/config/builder.rs`
- `client/src/config/validation.rs`
- `client/README.md`

### Changes
1. Add `UpdaterConfig` to `AppConfig`:
- `enabled: Option<bool>`
- `manifestUrl: Option<String>`
- `channel: Option<String>` (default `stable`)
- `checkIntervalMinutes: Option<u64>`

2. Extend `ResolvedConfig` with normalized updater fields:
- `updater_enabled: bool`
- `updater_manifest_url: Option<String>`
- `updater_channel: String`
- `updater_check_interval_minutes: u64`
- `updater_trust_anchors: &'static [(u64, &'static str)]` (compile-time map: chain id -> Safe address)

3. Add env overrides in `builder.rs`:
- `VIBEFI_UPDATER_ENABLED`
- `VIBEFI_UPDATER_MANIFEST_URL`
- `VIBEFI_UPDATER_CHANNEL`
- `VIBEFI_UPDATER_CHECK_INTERVAL_MINUTES`

4. Add validation in `validation.rs`:
- manifest URL must be `https://`.
- `updater_channel` must map to a supported manifest family (`stable`, `beta`, etc.).
- at startup, current `ResolvedConfig.chain_id` must have a baked trust-anchor Safe address (for production: `1`; for test flow: `11155111`).

## 2) New updater module
### Files
- `client/src/updater/mod.rs` (new)
- `client/src/updater/types.rs` (new)
- `client/src/updater/manifest.rs` (new)
- `client/src/updater/eip1271.rs` (new)
- `client/src/updater/trust_anchors.rs` (new)
- `client/src/updater/download.rs` (new)
- `client/src/updater/install.rs` (new)

### Changes
1. `types.rs`
- define serde structs for manifest + artifact entries.
- define `UpdateCheckResult`, `UpdateAvailability`, `UpdaterState`.

2. `manifest.rs`
- parse/validate manifest schema version.
- normalize artifact map.
- build deterministic `artifactsRoot`.
- compute EIP-712 digest using Alloy primitives.

3. `eip1271.rs`
- implement Safe ABI binding for `isValidSignature(bytes32,bytes)`.
- call `eth_call` via the same effective RPC URL used by client runtime (respecting existing endpoint selection/user override behavior).
- select expected Safe from baked trust-anchor map using `resolved.chain_id` (support `1` and `11155111`).
- enforce magic value `0x1626ba7e`.
- return rich errors: rpc, revert, invalid magic, malformed signature.

4. `trust_anchors.rs`
- expose `pub const TRUST_ANCHORS: &[(u64, &str)] = &[(1, \"0x...\"), (11155111, \"0x...\")]`.
- provide `fn safe_for_chain(chain_id: u64) -> Option<Address>`.

5. `download.rs`
- artifact download to staging dir.
- enforce max size from manifest.
- stream hash while downloading and compare SHA-256.

6. `install.rs`
- select artifact for current OS/arch.
- launch platform helper command and return `PendingRestart` state.

7. `mod.rs`
- public API:
  - `check_for_updates(state: Arc<AppState>, trigger: CheckTrigger) -> Result<UpdateCheckResult>`
  - `download_update(...)`
  - `apply_update_and_restart(...)`

## 3) Runtime integration
### Files
- `client/src/main.rs`
- `client/src/state.rs`

### Changes
1. Register `mod updater;` in `main.rs`.
2. Extend `AppState` with updater runtime:
- `updater_status: Arc<Mutex<UpdaterState>>`
- `updater_last_check: Arc<Mutex<Option<std::time::SystemTime>>>`

3. On startup (after config load), if updater enabled:
- spawn background thread/tokio runtime task for periodic checks using configured interval + jitter.
- first check after short delay (e.g. 20-60s), not blocking app launch.

4. Add new `UserEvent` variants in `state.rs`:
- `UpdaterStatusChanged { payload: ... }`
- `UpdaterCheckRequested`
- `UpdaterDownloadRequested { version: String }`
- `UpdaterApplyRequested { version: String }`

5. Handle updater events in the event loop in `main.rs` and dispatch to UI via host dispatch.

## 4) IPC + internal UI wiring
### Files
- `client/src/ipc_contract.rs`
- `client/src/ipc/mod.rs`
- `client/src/ipc/router.rs`
- `client/src/ipc/settings.rs` (or new `client/src/ipc/updater.rs`)
- `client/internal-ui/src/ipc/contracts.ts`
- `client/internal-ui/src/settings.tsx`
- `client/internal-ui/src/ipc/host-dispatch.ts`

### Changes
1. Add new provider id:
- Rust: `PROVIDER_ID_UPDATER = "vibefi-updater"`
- TS: include in `PROVIDER_IDS`.

2. Add IPC methods:
- `vibefi_getUpdaterStatus`
- `vibefi_checkForUpdates`
- `vibefi_downloadUpdate`
- `vibefi_applyUpdate`
- `vibefi_setUpdateChannel` (optional)

3. Add host dispatch message kind:
- `updaterStatus` with payload containing state/progress/errors.

4. Update settings UI:
- new “Updates” section with current version, channel, last check, available version, release notes.
- buttons: “Check now”, “Download”, “Restart to update”.
- disable actions while in-progress states.

## 5) Platform installer helpers
### Files
- `client/packaging/macos/install-vibefi-macos.sh` (existing)
- `client/packaging/windows/` (new helper script/binary wrapper)
- `client/packaging/linux/` (new helper script for appimage/deb behavior)
- `client/src/runtime_paths.rs`

### Changes
1. Add helper path resolution in `runtime_paths.rs`:
- `resolve_updater_helper()` per OS layout.

2. macOS helper:
- keep current staged replace strategy, but split into reusable helper that can accept a local downloaded artifact path and expected hash.

3. Windows helper:
- run signed installer (`msi`/`exe`) with safe args and wait for completion.

4. Linux helper:
- AppImage: atomic replace user-level appimage.
- Deb: invoke package manager flow and return “manual confirmation needed” status if privilege escalation required.

## 6) Release pipeline + manifest signer
### Files
- `.github/workflows/release.yml` (existing in release repo; update, do not create duplicate)
- `client/packaging/updater/build-manifest.mjs` (new, deterministic unsigned manifest builder)
- `client/packaging/updater/finalize-manifest.mjs` (new, inject Safe signature + final validation)
- `client/packaging/updater/verify-manifest.mjs` (new, local/CI verification helper)

### Changes
1. Split release into two explicit stages with a hard gate.
2. Stage 1 (`prepare-release`, CI):
- build artifacts as today.
- generate deterministic `manifest.unsigned.json`.
- compute and output canonical EIP-712 digest (`digest.txt`) and typed-data payload (`typed-data.json`) as workflow artifacts and release draft assets.
- stop before publish; no signature keys in CI.
3. Stage 2 (`finalize-release`, manual + CI resume):
- operator signs the Stage 1 digest using the Safe flow (onchain Safe transaction or Safe-compatible signature collection process).
- operator supplies resulting Safe signature bytes to CI via `workflow_dispatch` input or uploads a `signature.txt` artifact.
- CI runs `finalize-manifest.mjs` to produce `manifest.json` from `manifest.unsigned.json` + provided signature.
- CI verifies digest recomputation and onchain `isValidSignature` against the correct chain Safe.
- CI publishes assets only after verification passes.
4. Add guardrails:
- final publish job requires Stage 1 artifact hash match (prevents swapping unsigned payloads between stages).
- signature input format validation (hex length, prefix, structure).
- immutable linkage between release tag/version and unsigned manifest digest.
5. Add CI verification matrix:
- fixture checks for mainnet (`chainId=1`) and Sepolia (`chainId=11155111`) paths.
- production release path must verify against chain id declared in final manifest.

## 7) Download page alignment (`lander`)
### Files
- `lander/src/pages/Download.tsx`
- `client/packaging/macos/install-vibefi-macos.sh`

### Changes
1. Keep release repo constants unified across installer + download page (already aligned to `vibefi/client`; add CI guard to prevent drift).
2. Show “Update signature verified via Ethereum Safe (mainnet + Sepolia test support)” on download page.
3. Expose manifest URL/channel for transparency.

## Parallel Execution Plan (Agent Swarm)

## Track A: Core verifier
- A1: Implement manifest parsing + canonical hashing (`updater/types.rs`, `updater/manifest.rs`).
- A2: Implement EIP-1271 RPC verifier (`updater/eip1271.rs`).
- A3: Unit tests with known test vectors.

## Track B: Runtime + IPC
- B1: `AppState` and `UserEvent` updater additions (`state.rs`).
- B2: Event loop integration (`main.rs`).
- B3: IPC routes + contract updates (`ipc_contract.rs`, `ipc/router.rs`, `ipc/updater.rs`).

## Track C: UI
- C1: Extend TS IPC contracts.
- C2: Add Updates section in settings UI with all states.
- C3: Handle host dispatch progress updates.

## Track D: Installer helpers
- D1: macOS local-artifact helper integration.
- D2: Windows helper invocation.
- D3: Linux appimage/deb strategy.

## Track E: Release infra
- E1: Stage 1 release workflow (unsigned manifest + digest export).
- E2: manual Safe signing handoff + Stage 2 finalize workflow.
- E3: CI verification against mainnet + Sepolia `isValidSignature`.

Dependency edges:
- B depends on A public API.
- C depends on B IPC schema.
- D can proceed in parallel with A/B.
- E depends on A digest spec finalization.

## Test Plan (Required Gates)

## Unit tests
- `updater/manifest.rs`: deterministic digest for fixture manifest.
- `updater/eip1271.rs`: mock RPC responses for magic/non-magic/revert.
- config validation tests for updater fields.

## Integration tests
- local test fixture: signed manifest + tiny artifact; verify full flow check->download->hash pass.
- invalid signature should hard-fail before any download.
- hash mismatch should hard-fail install.

## E2E/manual acceptance
- startup auto-check does not block UI.
- check now / download / apply works in Settings.
- restart flow relaunches updated app.
- downgrade manifest rejected.

## Rollout Strategy
1. Ship with updater check-only mode (`download/apply` behind feature flag).
2. Enable download/apply for internal channel.
3. Enable stable channel after telemetry success threshold.
4. Keep emergency kill switch in manifest (`minSupportedVersion` + optional `blockedVersions`).

## Non-Negotiable Security Requirements
- No unsigned or invalidly signed manifest may trigger update download.
- Safe address must never be runtime-configurable in production binaries.
- EIP-1271 checks must use baked trust anchors for supported chains (`1`, `11155111`).
- EIP-1271 call transport must use the client's standard RPC selection path (default or user-provided).
- Artifact hash must match manifest before installer handoff.
- HTTPS-only manifest and artifact URLs.
- Do not disable OS code-signing/notarization checks.

## Suggested Crates
- `sha2` for SHA-256
- `semver` for version comparison
- `url` for strict URL validation
- keep using existing `reqwest` + `serde` + Alloy deps already present

## Done Definition
- New updater module merged with tests.
- Settings UI supports check/download/apply.
- Release pipeline is two-stage and emits signed manifests without CI-held signing keys.
- Client verifies EIP-1271 signature against baked Safe anchors (mainnet + Sepolia) before update acceptance.
- `README.md` documents updater config/env and operational runbook.
