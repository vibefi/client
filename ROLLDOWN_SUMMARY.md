# Rolldown Investigation - Quick Summary

## Can Rolldown Replace Bun?

**SHORT ANSWER**: Rolldown can replace some of Bun's usage, but not all.

## What Rolldown CAN Replace ✅

1. **Internal UI Bundling** (`internal-ui/scripts/build.ts`)
   - Multiple entry point bundling
   - TypeScript transpilation
   - Minification
   - Production builds

2. **Helper Scripts Bundling** (`walletconnect-helper`, `ipfs-helper`)
   - Bundle for Node.js/Bun runtime
   - Single file outputs

3. **Potentially Runtime Bundling** (`src/bundle.rs` - user dApps)
   - Could replace Vite, but not recommended yet
   - Wait for Rolldown maturity

## What Rolldown CANNOT Replace ❌

1. **Package Management**
   - `bun install` commands
   - Lock file management
   - **Alternative**: Use npm, pnpm, or yarn

2. **JavaScript Runtime**
   - Executing `.mjs` scripts at runtime
   - Running walletconnect-helper and ipfs-helper
   - **Alternative**: Use Node.js runtime instead of Bun

3. **Vendored Binary Distribution**
   - Currently vendor Bun binaries
   - **Alternative**: Vendor Node.js binaries (larger size)

## Recommendation

### ✅ DO: Phase 1 - Internal UI Migration
**Replace**: Bun bundling → Rolldown bundling (for internal-ui)  
**Keep**: Bun for package management & runtime (or switch to Node.js separately)  
**Benefit**: Faster builds, better Rust integration  
**Risk**: Low  
**Effort**: 1-2 weeks  

### 🤔 CONSIDER: Phase 2 - Helper Scripts Migration
**Replace**: Bun bundling → Rolldown bundling (for helpers)  
**Benefit**: Consistent tooling  
**Risk**: Low  
**Effort**: 1 week  

### ⏸️ WAIT: Runtime & Package Manager Changes
**Consider**: Switching from Bun runtime to Node.js  
**Decision**: Separate from Rolldown adoption  
**Reason**: Not required for bundling improvements  

### ❌ DON'T: Replace Vite (yet)
**Keep**: Vite for user dApp bundling  
**Reason**: Vite works well, Rolldown still in RC, complex migration  
**Future**: May reconsider when Rolldown 1.0+ is released  

## Key Insight

**Rolldown is a bundler, not a complete Bun replacement.**

Think of it as:
- ✅ Rolldown replaces: `bun build` commands
- ❌ Rolldown does NOT replace: `bun install` and `bun run` commands

## Integration Path

```
Current State:
  Bun → Package Management + Bundling + Runtime

Proposed State (Phase 1):
  Bun → Package Management + Runtime
  Rolldown → Bundling (internal-ui)

Optional Future State:
  npm/pnpm → Package Management
  Node.js → Runtime  
  Rolldown → Bundling
```

## Files Affected (Phase 1)

### To Create/Modify:
- `internal-ui/rolldown.config.js` (new)
- `internal-ui/package.json` (add rolldown)
- `internal-ui/scripts/build.ts` (migrate to Rolldown)
- `build.rs` (possibly update)
- `.github/workflows/*.yml` (install Rolldown)

### To Keep Unchanged:
- `src/bundle.rs` (keep Vite)
- `src/runtime_paths.rs` (keep Bun runtime)
- `vendor/` (keep Bun binaries)
- `Cargo.toml` (minimal changes)

## Next Step

👉 **Review `plan.md` for full details and decide whether to proceed with Phase 1.**

---
See `plan.md` for complete analysis, risk assessment, and implementation details.
