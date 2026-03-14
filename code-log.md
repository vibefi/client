# VibeFi Code — Compact Implementation Log (as of 2026-02-23)

## Summary

The code spike is no longer just a scaffold. The client now has a functional in-app "vibe-coding" environment with:

- Rust IPC backend for filesystem/project/dev-server/settings/validation flows
- Internal UI IDE shell (files, editor tabs, preview, console, chat)
- Claude/OpenAI streaming chat with tool calls (`read_file`, `write_file`, `delete_file`)
- Auto-applied file edits + diff viewing
- Dapp fork-to-code flow

Most of the original `code-spec.md` v1 stages (1-8) are implemented at a core level. Remaining work is mostly UX polish, missing edge features, security hardening, and tests.

## Current Implementation (Repo Reality)

### Rust side (`src/code/`)

- `router.rs`: `vibefi-code` IPC routing for file ops, project ops, dev server, settings, validation, forking, rename
- `filesystem.rs`: path validation/sandboxing + file operations
- `project.rs`: project scaffold/list/open + fork workflow
- `dev_server.rs`: `bun install`/Vite process lifecycle + output streaming + readiness detection + cleanup
- `settings.rs`: API keys / LLM config persistence
- `validator.rs`: project validation rules surfaced to UI/console
- `mod.rs`: module wiring into app state

### Internal UI (`internal-ui/src/code/`)

- `App.tsx`: main IDE orchestration (project, editor, preview, console, chat, settings)
- Hooks split by responsibility: `useProject`, `useDevServer`, `useEditor`, `useConsole`, `useSettings`, `useChat`
- Editor: tabbed code editor + console + diff viewer
- Preview: dev-server-backed iframe preview mode
- Chat: Claude/OpenAI streaming, tool-call rendering, diff integration
- LLM provider stack: `llm/claude.ts`, `llm/openai.ts`, `llm/provider.ts`, `llm/tools.ts`, `llm/system.ts`
- UI polish already added: file tree context menu + rename, compact side panels, editor tab scroll buttons

## Feature Status vs `code-spec.md`

### Implemented (core)

- Stage 1: Code tab foundation + IPC provider + filesystem security
- Stage 2: Project create/list/open + scaffold template
- Stage 3: Dev server start/stop/status + console streaming
- Stage 4: File tree + editor tabs + save flow + preview + console
- Stage 5: Chat UI + provider config + Claude/OpenAI streaming
- Stage 6: Tool-calling loop + auto-apply file edits + diff tab/viewer
- Stage 7: Fork dapp into Code and auto-open
- Stage 8: Validator core + console lint surfacing + general polish slices

### Notable follow-up fixes/polish already in repo

- Provider/model mismatch fixes in chat config flow
- Better dev-server subprocess/process-tree cleanup on app shutdown
- File tree node context menu + rename IPC
- Sidebar panel compaction
- Editor tab overflow handled with scroll buttons

### Important note

- `internal-ui/src/code/chat/llm/aiSdkTools.ts` exists but is not currently wired into `provider.ts` (orphaned adapter).

## What Is Still Missing (High-Level)

- File type icons in file tree (spec calls for distinct icons; tree is color-coded only)
- Recursive directory delete (`code_deleteDir`) + directory delete UI action
- Resizable pane dividers (sidebar / preview / chat split sizing)
- Persist last-opened project across sessions
- Full welcome/onboarding flow when API key is missing
- Chat access in `llm-preview` mode (currently hidden with editor shell)
- Automated tests (SSE parsing, stream/tool loop, diff formatting, etc.)
- Production-safe LLM key handling (proxy/ephemeral token path undecided)
- Tree background context menu (root-level new file/folder on empty-space right-click)

## Validation Snapshot (from recent log slices)

- `cargo check` in `client/`: passing (warnings only)
- `bunx tsc --noEmit` in `client/internal-ui/`: passing
- `bun run build` in `client/internal-ui/`: passing

## Source of Truth for Full Scope

- `code-spec.md`: original v1 design + stage plan + future enhancements
- `todo.md`: prioritized remaining work based on current repo state
