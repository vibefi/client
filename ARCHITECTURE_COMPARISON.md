# Bun vs Rolldown Architecture Comparison

## Current Architecture (Bun)

```
┌─────────────────────────────────────────────────────────────┐
│                     VibeFi Client Build                      │
└─────────────────────────────────────────────────────────────┘
                               │
                               ▼
                    ┌──────────────────┐
                    │    build.rs      │
                    │  (Rust build)    │
                    └──────────────────┘
                               │
                 ┌─────────────┴──────────────┐
                 ▼                            ▼
      ┌─────────────────┐          ┌──────────────────┐
      │ bun install     │          │ bun run build    │
      │ (dependencies)  │          │ (bundle build)   │
      └─────────────────┘          └──────────────────┘
                                           │
                                           ▼
                              ┌────────────────────────┐
                              │ internal-ui/scripts/   │
                              │ build.ts               │
                              │ (uses Bun.build API)   │
                              └────────────────────────┘
                                           │
                                           ▼
                              ┌────────────────────────┐
                              │ Bundle 9 entry points  │
                              │ → dist/*.js            │
                              └────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                     Runtime Architecture                     │
└─────────────────────────────────────────────────────────────┘

    User launches VibeFi → Rust binary (vibefi)
                                │
                                ├─→ Internal UI (built at compile time)
                                │
                                ├─→ User dApps bundling
                                │       │
                                │       ▼
                                │   ┌──────────────────┐
                                │   │ bundle.rs        │
                                │   │ calls Bun binary │
                                │   └──────────────────┘
                                │       │
                                │       ├─→ bun install
                                │       └─→ bun x --bun vite build
                                │
                                └─→ Helper Scripts
                                        │
                                        ├─→ walletconnect-helper.mjs
                                        │   (run with Bun)
                                        │
                                        └─→ ipfs-helper.mjs
                                            (run with Bun)

   Vendored Bun Binary: ~30MB
   - vendor/bun/bun-aarch64-apple-darwin
   - vendor/bun/bun-x86_64-apple-darwin  
   - vendor/bun/bun-x86_64-unknown-linux-gnu
```

## Proposed Architecture - Phase 1 (Rolldown for Internal UI)

```
┌─────────────────────────────────────────────────────────────┐
│                     VibeFi Client Build                      │
└─────────────────────────────────────────────────────────────┘
                               │
                               ▼
                    ┌──────────────────┐
                    │    build.rs      │
                    │  (Rust build)    │
                    └──────────────────┘
                               │
                 ┌─────────────┴──────────────┐
                 ▼                            ▼
      ┌─────────────────┐          ┌──────────────────┐
      │ bun install     │          │ bun run build    │ ← or node/npm
      │ (dependencies)  │          │ (bundle build)   │
      └─────────────────┘          └──────────────────┘
                                           │
                                           ▼
                              ┌────────────────────────┐
                              │ internal-ui/scripts/   │
                              │ build.ts               │
                              │ (uses Rolldown API) ★  │ ← CHANGE
                              └────────────────────────┘
                                           │
                                           ▼
                              ┌────────────────────────┐
                              │ Bundle 9 entry points  │
                              │ → dist/*.js            │
                              │ (Faster! Rust-based)   │
                              └────────────────────────┘

★ Rolldown replaces Bun.build() for bundling
  - Keeps using Bun/Node for script execution
  - Keeps using Bun/npm for package management

┌─────────────────────────────────────────────────────────────┐
│                     Runtime Architecture                     │
│                      (UNCHANGED)                             │
└─────────────────────────────────────────────────────────────┘

    User launches VibeFi → Rust binary (vibefi)
                                │
                                ├─→ Internal UI (built with Rolldown)
                                │   (faster build, same output)
                                │
                                ├─→ User dApps bundling
                                │   (still uses Bun + Vite)
                                │
                                └─→ Helper Scripts  
                                    (still run with Bun)

   Still vendor Bun Binary: ~30MB
```

## Proposed Architecture - Full Migration (Optional Future)

```
┌─────────────────────────────────────────────────────────────┐
│                     VibeFi Client Build                      │
└─────────────────────────────────────────────────────────────┘
                               │
                               ▼
                    ┌──────────────────┐
                    │    build.rs      │
                    │  (Rust build)    │
                    └──────────────────┘
                               │
                 ┌─────────────┴──────────────┐
                 ▼                            ▼
      ┌─────────────────┐          ┌──────────────────┐
      │ npm install     │          │ node build.ts    │ ← npm/node
      │ (dependencies)  │          │                  │
      └─────────────────┘          └──────────────────┘
                                           │
                                           ▼
                              ┌────────────────────────┐
                              │ Rolldown bundling      │
                              │ - internal-ui ★        │
                              │ - walletconnect ★      │
                              │ - ipfs-helper ★        │
                              └────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                     Runtime Architecture                     │
└─────────────────────────────────────────────────────────────┘

    User launches VibeFi → Rust binary (vibefi)
                                │
                                ├─→ Internal UI (Rolldown)
                                │
                                ├─→ User dApps bundling
                                │   (Rolldown or keep Vite)
                                │
                                └─→ Helper Scripts  
                                    (run with Node.js) ★

   Vendor Node.js Binary: ~50MB (larger)
   OR use system Node.js (0MB vendored)
```

## Decision Matrix

| Component | Current | Phase 1 | Full Migration |
|-----------|---------|---------|----------------|
| **Build System** |
| internal-ui bundling | Bun.build | **Rolldown** ✅ | Rolldown |
| helper scripts bundling | Bun.build | Bun.build | **Rolldown** |
| package management | Bun | Bun | npm/pnpm |
| **Runtime** |
| User dApp bundling | Bun+Vite | Bun+Vite | Rolldown or Vite |
| Helper script execution | Bun | Bun | Node.js |
| Vendored binary | Bun (30MB) | Bun (30MB) | Node.js (50MB) or none |
| **Performance** |
| Build speed | Fast | **Faster** 🚀 | Faster |
| Bundle size | Small | Similar | Similar |
| **Risk** |
| Implementation risk | N/A | **Low** ✅ | Medium |
| Compatibility risk | N/A | **Low** ✅ | Medium |

## Key Differences

### Bun
- **All-in-one**: Package manager + Bundler + Runtime
- **Fast**: Written in Zig, optimized for speed
- **Node-compatible**: Can run Node.js code
- **Smaller binary**: ~30MB vendored

### Rolldown  
- **Bundler only**: Just handles bundling
- **Very fast**: Written in Rust, Rollup-compatible
- **Requires runtime**: Needs Node.js or Bun to run
- **Focused**: Does one thing well

### Hybrid Approach (Recommended)
- **Rolldown**: For bundling (faster, Rust-native)
- **Bun or Node.js**: For package management and runtime
- **Best of both**: Performance + compatibility

## Migration Impact

### Phase 1: Internal UI Only
```
Lines of code changed: ~50-100
Files modified: 3-4
Build time improvement: 30-50%
Risk level: LOW
Reversibility: HIGH (easy to revert)
Time estimate: 1-2 weeks
```

### Full Migration
```
Lines of code changed: ~200-300
Files modified: 10-15
Build time improvement: 40-60%
Risk level: MEDIUM
Reversibility: MEDIUM (requires more work)
Time estimate: 1-2 months
```

## Recommendation Summary

```
┌──────────────────────────────────────────────────┐
│  START HERE: Phase 1 - Internal UI Migration    │
│                                                  │
│  ✅ Low risk                                     │
│  ✅ High value (faster builds)                   │
│  ✅ Easy to test and validate                    │
│  ✅ Reversible if issues                         │
│  ✅ Aligns with Rust-native tooling              │
└──────────────────────────────────────────────────┘

Then decide based on results:
  → Success? Proceed to Phase 2 (helper scripts)
  → Issues? Revert and reassess
  → Works great? Consider full migration later
```

---
**See plan.md for detailed implementation steps**
