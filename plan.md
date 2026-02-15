# Rolldown Integration Plan for VibeFi Client

## Executive Summary

This document outlines the analysis and integration strategy for replacing Bun with Rolldown, a Rust-based JavaScript/TypeScript bundler. After analyzing the current Bun usage in the VibeFi client repository, we've identified both opportunities and limitations for Rolldown adoption.

## Current Bun Usage Analysis

### 1. Build-Time Bundling (build.rs)
- **Location**: `build.rs`
- **Purpose**: Builds the internal-ui React application during Cargo build
- **Commands**: 
  - `bun install` - Install dependencies
  - `bun run build` - Execute the build script
- **Can Rolldown Replace?**: **Partially** - Rolldown can replace the bundling, but not the package management

### 2. Internal UI Build Script (internal-ui/scripts/build.ts)
- **Location**: `internal-ui/scripts/build.ts`
- **Purpose**: Bundle multiple entry points (preload scripts, React pages)
- **Features Used**:
  - Multiple entry points bundling
  - IIFE format for browser injection
  - Minification
  - Production builds with environment variable injection
  - TypeScript transpilation
- **Can Rolldown Replace?**: **YES** - This is Rolldown's primary use case

### 3. Runtime Bundling (src/bundle.rs)
- **Location**: `src/bundle.rs`
- **Purpose**: Build user dApps at runtime using Vite
- **Commands**:
  - `bun install --no-save` - Install dependencies
  - `bun x --bun vite build` - Run Vite bundler
- **Can Rolldown Replace?**: **Partially** - Rolldown can bundle, but Vite integration is complex

### 4. Helper Scripts Bundling
- **Locations**: `walletconnect-helper/package.json`, `ipfs-helper/package.json`
- **Purpose**: Bundle Node.js scripts for distribution
- **Commands**: `bun build index.mjs --bundle --target=bun --conditions=module`
- **Can Rolldown Replace?**: **YES** - Rolldown can bundle for Node.js runtime

### 5. Package Management
- **All locations**: `bun install`, `bun.lock` files
- **Purpose**: Dependency installation and lock file management
- **Can Rolldown Replace?**: **NO** - Rolldown is not a package manager

### 6. Runtime Execution
- **Location**: `src/runtime_paths.rs`, `src/walletconnect.rs`, `src/ipfs_helper.rs`
- **Purpose**: Execute JavaScript with Bun runtime (as Node.js alternative)
- **Can Rolldown Replace?**: **NO** - Rolldown is a bundler, not a runtime

## Rolldown Capabilities

### Strengths
- **Rollup-compatible API**: Drop-in replacement for many Rollup use cases
- **Fast Performance**: Written in Rust, significantly faster than JavaScript bundlers
- **Multiple Output Formats**: ESM, CJS, IIFE, etc.
- **TypeScript Support**: Native TypeScript transpilation
- **Tree Shaking**: Advanced dead code elimination
- **Plugin System**: Rollup-compatible plugin interface
- **Source Maps**: Full source map support
- **Code Splitting**: Supports dynamic imports and chunk splitting
- **Minification**: Built-in (alpha status)

### Limitations
- **RC Status**: Currently in Release Candidate, not fully stable
- **Minification Alpha**: Built-in minification still in alpha
- **Not a Package Manager**: Cannot replace `bun install` or manage dependencies
- **Not a Runtime**: Cannot execute JavaScript code like Bun or Node.js
- **Limited Vite Integration**: No direct Vite compatibility layer yet
- **Community/Ecosystem**: Smaller plugin ecosystem compared to established bundlers

## Integration Strategy

### Phase 1: Internal UI Build (Low Risk, High Value)
**Target**: Replace Bun bundling in `internal-ui/scripts/build.ts`

**Benefits**:
- Removes one Bun usage point
- Faster builds due to Rust performance
- Better integration with Rust toolchain
- Still in Cargo build context

**Implementation**:
1. Add Rolldown as npm dependency in `internal-ui/`
2. Create Rolldown config file (`rolldown.config.js`)
3. Migrate build script from Bun.build() to Rolldown API
4. Update `build.rs` to call Node.js/npm instead of Bun for build step
5. Keep `bun install` for dependency management (for now)

**Challenges**:
- Still need Bun/npm for package management
- Need Node.js runtime to execute Rolldown (unless compiled to Rust binary)
- Testing parity with current Bun build output

**Alternative Approach**: Use Rolldown's Rust API directly from `build.rs`
- Requires Rolldown Rust crate (if available)
- Eliminates Node.js/Bun runtime dependency for builds
- More complex integration but cleaner for Rust project

### Phase 2: Helper Scripts Bundling (Low Risk, Medium Value)
**Target**: Replace Bun bundling in `walletconnect-helper` and `ipfs-helper`

**Benefits**:
- Consistent tooling across project
- Better integration with package process

**Implementation**:
1. Add Rolldown to each helper's package.json
2. Update `build:dist` scripts to use Rolldown CLI or API
3. Configure for Node.js/Bun target output
4. Update `Cargo.toml` packaging commands

**Challenges**:
- Must ensure output is compatible with both Node.js and Bun runtimes
- Existing bundles work, so less urgent

### Phase 3: Runtime Bundling Investigation (High Risk, Low Priority)
**Target**: Explore replacing Vite in `src/bundle.rs`

**Why Low Priority**:
- Vite is well-established and works well
- Users' dApps may rely on Vite-specific features
- Complex migration with limited benefit
- Rolldown is designed to eventually be Vite's bundler, so waiting may be better

**Potential Approach** (if pursued):
- Replace `bun x --bun vite build` with Rolldown-based build
- Would need to implement:
  - React plugin equivalent
  - Dev server capabilities (if needed)
  - Full Vite config compatibility
  - HMR support (for dev mode)

**Recommendation**: **Wait** - Let Rolldown mature and potentially integrate into Vite upstream

### What Cannot Be Replaced

1. **Package Management** (`bun install`)
   - **Alternative**: Switch to npm/pnpm/yarn for package management
   - Keep using lockfiles appropriate to chosen package manager

2. **JavaScript Runtime** (executing scripts)
   - **Alternative**: Use Node.js instead of Bun runtime
   - Update `src/runtime_paths.rs` to use Node.js binary
   - Less critical since Bun is Node-compatible in most cases

3. **Vendored Binary Distribution**
   - **Current**: Vendor Bun binary with application
   - **Alternative with Rolldown**: Vendor Node.js binary instead
   - **Impact**: Larger binary size (Node.js ~50MB vs Bun ~30MB)

## Recommended Rolldown Adoption Path

### Immediate/Short-term (Weeks 1-2)
```
Phase 1A: Internal UI Build with Rolldown CLI
- Add rolldown to internal-ui/package.json
- Create rolldown.config.js
- Migrate build.ts to use Rolldown
- Keep bun install for dependency management
- Update build.rs to invoke the new build
```

**Why Start Here?**:
- Self-contained change
- Easy to test and validate
- Quick wins for build speed
- Reversible if issues arise

### Medium-term (Weeks 3-4)
```
Phase 1B: Helper Scripts Migration
- Update walletconnect-helper bundling
- Update ipfs-helper bundling
- Test with existing consumers
```

### Optional/Future Consideration
```
Phase 2: Runtime Migration (Node.js)
- Replace Bun runtime with Node.js
- Update runtime_paths.rs resolution
- Test walletconnect-helper and ipfs-helper with Node.js
- Update vendor scripts to fetch Node.js binaries
```

```
Phase 3: Package Manager Decision
- Evaluate npm vs pnpm vs bun for dependency management
- Update all package.json scripts
- Update CI/CD workflows
- Update developer documentation
```

## Risk Assessment

### High Risk Areas
- **Bundling Parity**: Ensuring Rolldown produces functionally equivalent output to Bun
- **Runtime Compatibility**: If switching from Bun to Node.js runtime
- **Build System Changes**: Modifications to build.rs could affect all builds
- **Plugin Compatibility**: If any Rollup plugins don't work with Rolldown

### Mitigation Strategies
1. **Incremental Adoption**: Change one component at a time
2. **Testing**: Extensive testing of bundled output
3. **Feature Flags**: Environment variable to switch between Bun and Rolldown
4. **Rollback Plan**: Keep Bun as fallback option initially
5. **Monitoring**: Track build times, bundle sizes, runtime behavior

## Performance Expectations

### Build Times (estimated)
- **Internal UI Build**: 30-50% faster with Rolldown (Rust vs JavaScript)
- **Helper Scripts**: 40-60% faster with Rolldown
- **Full Cargo Build**: 5-10% faster overall (UI build is portion of total)

### Bundle Sizes
- Should be similar or slightly smaller with Rolldown
- Better tree shaking may reduce size further

### Runtime Performance
- Bundle output should have no runtime performance difference
- Execution speed determined by browser/Node.js, not bundler

## Development Environment Impact

### Current Requirements
- Bun must be installed for development
- Bun handles both building and runtime execution

### After Rolldown Migration (Phase 1)
- **Option A**: Keep Bun for package management + runtime, use Rolldown for bundling
  - Developers still need Bun
  - Adds Rolldown as additional dependency
  
- **Option B**: Switch to npm + Node.js, use Rolldown for bundling
  - Developers need Node.js instead of Bun
  - More standard JavaScript toolchain
  - Familiar to most developers

### Recommendation
Start with Option A (minimal disruption), consider Option B for Phase 2

## Conclusion

### Summary
Rolldown can **partially replace** Bun in the VibeFi client:

✅ **Can Replace**: Build-time bundling (internal-ui, helpers)
❌ **Cannot Replace**: Package management, JavaScript runtime execution
⚠️ **Could Replace**: Runtime bundling (but not recommended yet)

### Recommended Approach
**Hybrid Strategy**: Use Rolldown for bundling, keep Bun (or switch to Node.js) for package management and runtime execution.

**Priority**: Focus on internal-ui build migration first, as it provides the clearest benefits with lowest risk.

### Decision Points
1. **Adopt Rolldown for internal-ui builds?** → **Yes (Recommended)**
   - Clear value proposition
   - Low risk
   - Aligns with Rust-based tooling

2. **Replace Bun runtime with Node.js?** → **Evaluate separately**
   - Not required for Rolldown adoption
   - Consider based on team preferences
   - Bun is faster, Node.js is more established

3. **Full Bun removal?** → **Not feasible** (need package manager and runtime)

4. **Wait for Rolldown 1.0?** → **Not necessary for Phase 1**
   - RC is stable enough for build tools
   - Can start with internal use
   - Upgrade to 1.0 when available

## Next Steps

If proceeding with Rolldown integration:

1. ✅ Review and approve this plan
2. Create proof-of-concept for internal-ui build with Rolldown
3. Compare build output and performance metrics
4. Update CI/CD to install Rolldown
5. Implement Phase 1A with feature flag
6. Test thoroughly in development and CI
7. Document changes for team
8. Merge and monitor
9. Iterate to Phase 1B based on results

## Appendix: Code Changes Overview

### Files to Modify (Phase 1A)
```
internal-ui/
  ├── package.json           [add rolldown dependency]
  ├── rolldown.config.js     [new file]
  └── scripts/
      └── build.ts           [migrate to Rolldown API]

build.rs                     [possibly update invocation]

.github/workflows/
  └── build.yml              [install Rolldown if needed]
```

### Files to Modify (Phase 1B)  
```
walletconnect-helper/
  ├── package.json           [add rolldown, update script]
  └── rolldown.config.js     [new file]

ipfs-helper/
  ├── package.json           [add rolldown, update script]
  └── rolldown.config.js     [new file]

Cargo.toml                   [update before-packaging-command]
```

### Files to Keep Unchanged
```
src/bundle.rs                [keep Vite for now]
src/runtime_paths.rs         [keep Bun runtime for now]
vendor/                      [keep Bun binaries for now]
```

## Alternative: Rust-Native Approach

### Using oxc or swc Directly
Instead of Rolldown (which still needs Node.js to run), consider using pure Rust bundlers:

**Options**:
- [oxc](https://oxc-project.github.io/) - Rust-based JavaScript oxidation compiler
- [swc](https://swc.rs/) - Speedy web compiler written in Rust
- [rspack](https://www.rspack.dev/) - Rust-based Webpack alternative

**Advantages**:
- No Node.js/Bun runtime needed for builds
- True Rust-to-Rust integration in build.rs
- Potentially even faster builds
- Single language ecosystem

**Disadvantages**:
- Less mature than Rolldown for bundling
- May not have all required features yet
- More complex API integration
- Smaller community

**Recommendation**: Worth investigating as alternative to Rolldown, especially for Phase 1A where we control the build script entirely.

---

**Document Version**: 1.0  
**Date**: 2026-02-15  
**Author**: GitHub Copilot  
**Status**: Pending Review
