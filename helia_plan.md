# Helia Verified Fetch Implementation Plan

## 1. Goal

Add an optional IPFS retrieval backend that does not require a local IPFS node, using a JS helper process built around `@helia/verified-fetch`, while preserving current launch behavior and safety checks.

## 2. Scope

In scope:
- Optional backend selection between current gateway+local-node path and Helia helper path.
- Retrieval of `manifest.json` and all bundle files for a root CID.
- Integrity checks that do not depend on local Kubo.
- Packaging/runtime wiring for the helper (dev + release builds).
- Tests for success/failure paths and fallback behavior.

Out of scope for first pass:
- Replacing all IPFS-related functionality with Helia (only bundle retrieval path).
- IPNS/DNSLink support.
- Full performance optimization and background prefetching.

## 3. Current Baseline (for reference)

- Launch path: `internal-ui` calls `vibefi_launchDapp`.
- Rust backend downloads from `{ipfsGateway}/ipfs/{rootCid}/...` in `src/registry.rs`.
- Manifest is validated via `verify_manifest` in `src/bundle.rs`.
- Root CID is re-computed via local IPFS API (`/api/v0/add`, `only-hash=true`) in `compute_ipfs_cid`, requiring local `ipfsApi`.

Relevant files:
- `src/registry.rs`
- `src/config.rs`
- `src/bundle.rs`
- `src/runtime_paths.rs`
- `src/walletconnect.rs` (pattern to mirror)
- `walletconnect-helper/` (helper architecture to mirror)

## 4. Target Architecture

### 4.1 High level

Add a new helper process, `ipfs-helper`, that exposes line-delimited JSON-RPC over stdin/stdout (same style as WalletConnect helper). Rust calls helper methods to retrieve verified content for:
- `ipfs://<rootCid>/manifest.json`
- `ipfs://<rootCid>/<entry.path>` for each manifest file

### 4.2 Retrieval trust model

- Use `@helia/verified-fetch` for content retrieval and per-request verification.
- Keep existing manifest structure checks (`files` list non-empty, file existence, byte-size checks).
- Remove hard dependency on local `ipfsApi` for launch path.
- Optional: keep legacy local-node CID recomputation as a configurable strict mode during migration.

### 4.3 Backend selection

Introduce config-driven backend selection:
- `gateway` (existing behavior)
- `helia` (new helper behavior)

Default recommendation:
- Keep `gateway` default for one release (safe migration), then flip default to `helia` after validation.

## 5. Detailed Implementation Plan

## Phase 0: Design + Contracts

1. Define an `IpfsFetchBackend` enum in Rust (`Gateway`, `Helia`).
2. Add config keys (with defaults):
   - `ipfsFetchBackend`: `"gateway" | "helia"`
   - `ipfsHeliaGateways`: optional list of trustless gateways
   - `ipfsHeliaRouters`: optional list of delegated routers
   - `ipfsHeliaTimeoutMs`: request timeout
   - `ipfsStrictRootVerification`: bool (migration toggle)
3. Define helper RPC schema:
   - `ping`
   - `fetch` with params `{ url: string, timeoutMs?: number }`
   - result `{ status: number, headers: Record<string,string>, bodyBase64: string }`
4. Define Rust-side error taxonomy:
   - helper spawn failure
   - helper protocol failure
   - verified fetch non-success status
   - manifest parse/validation failure

Deliverable:
- Small RFC section in `helia_plan.md` finalized into implementation notes.

## Phase 1: Add JS Helper Package

1. Create `ipfs-helper/` package modeled after `walletconnect-helper/`:
   - `package.json`
   - `index.mjs`
   - build script to emit `dist/ipfs-helper.mjs`
2. Install dependencies:
   - `@helia/verified-fetch`
   - (if needed for explicit customization) `@helia/http`, `@helia/block-brokers`, `@helia/routers`
3. Implement RPC loop:
   - read line-delimited JSON from stdin
   - execute method
   - write single-line JSON response
4. Implement `fetch` method:
   - call verified fetch on the provided `ipfs://` URL
   - return status + headers + base64 body
   - include structured error payloads
5. Add startup diagnostics to stderr only (never stdout).

Deliverable:
- `ipfs-helper` runnable with `node ipfs-helper/index.mjs` and a passing `ping`.

## Phase 2: Rust Runtime Path + Bridge

1. Add runtime resolver in `src/runtime_paths.rs`:
   - `resolve_ipfs_helper_script()` using same precedence pattern as walletconnect helper
   - env override `VIBEFI_IPFS_HELPER_SCRIPT`
2. Add Rust bridge module `src/ipfs_helper.rs`:
   - `IpfsHelperBridge::spawn(...)`
   - `ping()`
   - `fetch(url)`
   - request/response id matching
   - robust parsing and IO error handling
3. Keep protocol implementation parallel to `src/walletconnect.rs` for consistency.

Deliverable:
- Unit tests for JSON line parsing and mismatch/error handling.

## Phase 3: Config Wiring

1. Update `AppConfig` and `NetworkContext` in `src/config.rs`:
   - new backend + helper settings
   - sane defaults for gateways/routers/timeouts
2. Ensure backwards compatibility for existing config files (`mainnet.json`, `sepolia.json`).
3. Add config validation:
   - reject unknown backend values
   - reject empty gateway list when backend is `helia` and explicit override is present

Deliverable:
- App boots with both backend choices and no changes required to old config files.

## Phase 4: Retrieval Path Refactor in `registry.rs`

1. Refactor `ensure_bundle_cached` into strategy-dispatched flow:
   - `ensure_bundle_cached_gateway(...)` (existing code path)
   - `ensure_bundle_cached_helia(...)` (new helper path)
2. Implement Helia retrieval:
   - fetch manifest via helper from `ipfs://<rootCid>/manifest.json`
   - parse and validate manifest structure
   - fetch each file from `ipfs://<rootCid>/<path>`
   - write to cache dir preserving hierarchy
3. Keep `verify_manifest(&bundle_dir)` unchanged as post-download check.
4. Root CID validation strategy:
   - initial migration: skip local `compute_ipfs_cid` when backend is `helia`
   - if `ipfsStrictRootVerification=true`, optionally run legacy compute path when local API exists
   - later phase: replace strict mode with node-independent root re-verification if needed

Deliverable:
- Launch flow succeeds with `helia` backend and no local Kubo.

## Phase 5: Packaging + Build Integration

1. Mirror walletconnect packaging for `ipfs-helper` artifact:
   - include helper script in app resources for macOS/Linux packages
2. Ensure development fallback path works from source tree.
3. Update build pipeline docs and CI setup:
   - install helper dependencies
   - bundle helper script into release artifact

Deliverable:
- `cargo run` in dev and packaged apps both resolve helper reliably.

## Phase 6: Tests

### 6.1 Rust unit tests
- Config parsing defaults and backend selection.
- Helper protocol parser behavior.
- URL construction and file write path safety.

### 6.2 Integration tests
- Mock helper returns known manifest/files -> launch succeeds.
- Helper returns 404 for file -> launch fails with clear error.
- Corrupt manifest -> parse/validation error.
- Byte mismatch in downloaded file -> `verify_manifest` failure.

### 6.3 Manual tests
- Run without local Kubo and launch a published dapp.
- Kill helper mid-request and confirm clear recovery/error path.
- Verify cache hit behavior still works.

Deliverable:
- Test checklist added to PR template or release checklist.

## Phase 7: Observability + Failure UX

1. Add explicit backend logs:
   - `[ipfs] backend=helia` / `[ipfs] backend=gateway`
2. Add concise failure messages surfaced in launcher log panel.
3. Add timing metrics (manifest fetch duration, total bundle fetch duration).

Deliverable:
- Faster debugging for retrieval issues and gateway/helper outages.

## Phase 8: Rollout Strategy

1. Release N:
   - Helia backend shipped behind config flag.
   - Default remains `gateway`.
2. Release N+1 (after validation):
   - Default to `helia`.
   - Keep gateway backend as fallback.
3. Release N+2:
   - Re-evaluate deprecating local-node-dependent strict verification path.

Deliverable:
- Low-risk migration with clear rollback path.

## 11. Concrete File-Level Changes

Add:
- `ipfs-helper/package.json`
- `ipfs-helper/index.mjs`
- `src/ipfs_helper.rs`

Modify:
- `src/main.rs` (module wiring + state if needed)
- `src/runtime_paths.rs` (helper script resolution)
- `src/config.rs` (new config knobs)
- `src/registry.rs` (backend dispatch + helia fetch path)
- `build.rs` / packaging config (include helper artifact)
- `README.md` (new setup/runtime docs)
- `config/mainnet.json`, `config/sepolia.json` (optional explicit backend key)

## 12. Implementation Complexity Estimate

- Helper package + Rust bridge: Medium
- Registry refactor + verification policy: Medium
- Packaging/build integration: Medium
- Test coverage: Medium
- Overall: Medium-high (primarily due to process boundary + rollout safety)

## 13. Risks and Mitigations

1. Dependency churn in Helia ecosystem.
- Mitigation: pin exact versions initially; schedule periodic upgrades.

2. Helper process runtime issues (spawn/path/IO deadlocks).
- Mitigation: reuse WalletConnect bridge patterns and timeout discipline.

3. Verification semantics drift vs current root-CID recomputation.
- Mitigation: keep strict-mode migration toggle and explicit documentation of trust model.

4. Public gateway quality variability.
- Mitigation: configurable gateway set, retries, and clear error categorization.

## 14. Acceptance Criteria

1. With no local IPFS node running, launching a published dapp works end-to-end using `ipfsFetchBackend=helia`.
2. Bundle content is still validated via manifest checks before build/launch.
3. Existing gateway path still works unchanged when selected.
4. Packaged app can locate and run `ipfs-helper` without manual setup.
5. Failures are actionable in launcher logs.

## 15. Suggested Execution Order (PR slices)

1. PR1: `ipfs-helper` package + Rust bridge + runtime path resolver.
2. PR2: Config model and backend selector.
3. PR3: `registry.rs` Helia retrieval strategy + tests.
4. PR4: Packaging/build integration + docs.
5. PR5: rollout default switch (optional, after production validation).
