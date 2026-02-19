
## Implementation Progress Log

Date: 2026-02-17

Completed so far in repo:

- Added a new Rust `code` module scaffold: `src/code/mod.rs`, `src/code/router.rs`, `src/code/filesystem.rs`, `src/code/project.rs`.
- Added `vibefi-code` provider wiring in IPC contracts/router.
- Implemented initial filesystem IPC methods for Code:
  - `code_listFiles`
  - `code_readFile`
  - `code_writeFile`
  - `code_deleteFile`
  - `code_createDir`
- Implemented file/path safety protections in Code filesystem layer:
  - Relative-path normalization and traversal checks
  - Blocked sensitive paths (`node_modules`, `.vibefi`, dot-directories)
  - Write extension allowlist (`.ts`, `.tsx`, `.css`, `.json`, `.html`, `.webp`)
  - Symlink-aware path anchoring checks
- Added initial `CodeState` fields in app state.
- Added an internal Code UI entrypoint and static host page:
  - `internal-ui/src/code.tsx`
  - `internal-ui/static/code.html`
- Updated internal UI build wiring to emit `dist/code.js`.
- Updated embedded asset wiring and app webview serving for Code content.
- Added a Code-specific CSP branch in webview serving.
- Updated startup flow so Code is available in non-bundle flows and is not opened in `--bundle` mode.
- Current baseline compiles with `cargo check` (warnings present, no build errors).

Open follow-up work in progress:

- Keep Studio behavior isolated and avoid unnecessary Studio-path changes while continuing Code implementation.
- Continue Stage 1/2 implementation: project management IPC (`create/list/open`) and stronger end-to-end Code tab startup behavior.

### Progress Update 2 (continued)

- Added project management IPC implementation in `src/code/project.rs` and `src/code/router.rs`:
  - `code_createProject`
  - `code_listProjects`
  - `code_openProject`
- Added scaffold/template generation for new projects, plus project root validation.
- Added Code dev server foundation in `src/code/dev_server.rs` and IPC routes:
  - `code_startDevServer`
  - `code_stopDevServer`
  - `code_devServerStatus`
- Added one-process dev server state in `CodeState` and startup wiring in app state.
- Added provider event streaming for Code runtime events:
  - `codeConsoleOutput`
  - `codeDevServerReady`
  - `codeDevServerExit`
- Updated `internal-ui/src/code.tsx` with:
  - project list/create/open flow
  - basic file-tree JSON display
  - dev server start/stop/status controls
  - live console stream for Code provider events
- Confirmed build status:
  - `cargo check` succeeds in `client/`
  - `bun run build` succeeds in `client/internal-ui/`

### Progress Update 3 (preview + default file open)

- Added Code UI preview panel with iframe bound to the running dev server URL (`http://localhost:<port>`).
- Added default file open behavior after project open/create:
  - first tries `src/App.tsx`
  - falls back to `index.html` if needed
- Added read-only file viewer panel showing opened file path + content.
- Added manual file path input/button in Code UI to read another file via `code_readFile`.
- Added a minimal navigation-policy adjustment for Code webviews so localhost preview iframe navigation is allowed while keeping non-Code webviews unchanged.
- Re-validated:
  - `cargo check` passes in `client/`
  - `bun run build` passes in `client/internal-ui/`

### Progress Update 4 (tabbed editor + save flow + file-change sync)

- Replaced the read-only file viewer in `internal-ui/src/code.tsx` with a tabbed Code workspace model:
  - recursive clickable file tree (directory expand/collapse)
  - always-present Console tab + closeable file tabs
  - active tab switching, close behavior, and active-file highlighting in the tree
- Added editable file-tab buffers with save lifecycle:
  - dirty tracking (content vs last saved content)
  - explicit Save action for the active file tab
  - `Ctrl+S` / `Cmd+S` keyboard save
  - debounce auto-save on editor blur and on tab switch away from dirty file tabs
- Kept existing project/dev-server/preview flows intact while integrating with the new editor model:
  - default file open now populates file tabs (`src/App.tsx` fallback `index.html`)
  - manual open-file path input now opens/focuses tabs
  - event handling for `codeConsoleOutput` now accepts both payload styles (`source` and legacy `stream`)
- Added Rust-side `codeFileChanged` event emission from `vibefi-code` filesystem mutations in `src/code/router.rs`:
  - emits on `code_writeFile` (`modify`)
  - emits on `code_deleteFile` (`delete`)
  - emits on `code_createDir` (`create`)
- Wired `codeFileChanged` handling in UI:
  - refreshes file tree on change events
  - closes tabs when a file is externally deleted
  - refreshes clean/non-saving open file tabs from disk when externally modified
- Re-validated after this slice:
  - `cargo check` passes in `client/`
  - `bun run build` passes in `client/internal-ui/`

### Progress Update 5 (preview runtime error forwarding)

- Added preview runtime error forwarding script to new project scaffold `index.html` template in `src/code/project.rs`:
  - forwards `window.error` and `unhandledrejection` events to parent via `postMessage`
  - payload type: `vibefi-code-error` with message (+ stack when available)
- Added Code UI runtime listener in `internal-ui/src/code.tsx`:
  - listens for `message` events from `http://localhost:*` preview origins
  - consumes `vibefi-code-error` payloads and appends `[runtime]` entries to the Console tab output
- Re-validated after this slice:
  - `cargo check` passes in `client/`
  - `bun run build` passes in `client/internal-ui/`

### Progress Update 6 (console payload alignment)

- Updated Code dev-server event payloads in `src/code/dev_server.rs` to include `source` alongside existing `stream` for backward compatibility.
  - startup/install notices now emit `source: "system"`
  - stdout lines map to `source: "vite"`
  - stderr lines map to `source: "build"`
- This keeps current UI behavior working while aligning `codeConsoleOutput` shape with the spec direction.
- Re-validated after this slice:
  - `cargo check` passes in `client/`
  - `bun run build` passes in `client/internal-ui/`

### Progress Update 7 (CodeMirror editor integration)

- Added CodeMirror dependencies to `internal-ui` package:
  - `@codemirror/view`, `@codemirror/state`, `@codemirror/commands`, `@codemirror/language`
  - `@codemirror/lang-javascript`, `@codemirror/lang-json`, `@codemirror/lang-html`, `@codemirror/lang-css`
  - `@codemirror/search`, `@codemirror/theme-one-dark`
- Replaced file-tab textarea editor with a CodeMirror wrapper component in `internal-ui/src/code.tsx`:
  - per-file language extension detection (`.ts/.tsx/.js/.jsx/.json/.html/.css`)
  - one-dark styling and line numbers
  - change propagation wired to existing dirty/save state model
  - blur hook preserved for existing debounce autosave behavior
  - read-only mode while save is in progress
- Kept existing Console tab and project/dev-server flows unchanged.
- Re-validated:
  - `cargo check` passes in `client/`
  - `bun run build` passes in `client/internal-ui/`

### Progress Update 8 (console navigation + write event accuracy)

- Added clickable console path parsing in `internal-ui/src/code.tsx`:
  - detects `file:line` patterns in Console output
  - click opens/focuses file tab and jumps CodeMirror cursor to the referenced line
- Added line-jump support in CodeMirror wrapper in `internal-ui/src/code.tsx` (selection + scroll + focus).
- Updated Rust filesystem write signaling:
  - `src/code/filesystem.rs` now returns write kind (`Create` vs `Modify`) for `write_file`
  - `src/code/router.rs` now emits `codeFileChanged.kind` accurately from `code_writeFile`
- Re-validated:
  - `cargo check` passes in `client/`
  - `bun run build` passes in `client/internal-ui/`

### Progress Update 9 (file operations in Files panel)

- Added basic file operation controls to the Code Files panel in `internal-ui/src/code.tsx`:
  - `New File` (creates via `code_writeFile` with empty content, then opens tab)
  - `New Folder` (creates via `code_createDir`)
  - `Delete File` (deletes via `code_deleteFile` with confirmation)
- These controls integrate with existing tab state + file tree refresh flow.
- Re-validated:
  - `cargo check` passes in `client/`
  - `bun run build` passes in `client/internal-ui/`

### Progress Update 10 (layout chat-pane placeholder)

- Added a bottom LLM chat pane placeholder in `internal-ui/src/code.tsx` to match the intended Stage-4 layout structure:
  - collapsible panel (`Expand` / `Collapse`)
  - resizable chat shell area
  - placeholder history and disabled input/send controls (no provider logic yet)
- This is UI-only scaffolding and does not alter runtime/provider behavior.
- Re-validated:
  - `cargo check` passes in `client/`
  - `bun run build` passes in `client/internal-ui/`

### Progress Update 11 (quick open)

- Added quick-open file picker in `internal-ui/src/code.tsx`:
  - shortcut: `Ctrl+P` / `Cmd+P`
  - query filter over current project file tree
  - keyboard navigation (Up/Down/Enter/Escape)
  - opens selected file directly into the tabbed editor flow
- Re-validated:
  - `cargo check` passes in `client/`
  - `bun run build` passes in `client/internal-ui/`

### Progress Update 12 (command palette stub)

- Added command palette stub overlay in `internal-ui/src/code.tsx`:
  - shortcut: `Ctrl+Shift+P` / `Cmd+Shift+P`
  - escape/outside-click close behavior
  - intentionally empty command list placeholder (UI stub only)
- Re-validated:
  - `cargo check` passes in `client/`
  - `bun run build` passes in `client/internal-ui/`

### Progress Update 13 (code settings IPC methods)

- Wired Code settings IPC methods in `src/code/router.rs` using existing `src/code/settings.rs` persistence:
  - `code_getApiKeys`
  - `code_setApiKeys`
  - `code_getLlmConfig`
  - `code_setLlmConfig`
- Added request payload parsing and non-empty validation for LLM config (`provider`, `model`).
- Aligned default Code LLM model in `src/code/settings.rs` to `claude-sonnet-4-5-20250929`.
- Re-validated:
  - `cargo check` passes in `client/`
  - `bun run build` passes in `client/internal-ui/`

### Progress Update 14 (Code settings UI scaffold)

- Extended Code chat placeholder panel in `internal-ui/src/code.tsx` with persisted settings controls:
  - Claude API key input
  - OpenAI API key input
  - provider selector (`claude` / `openai`)
  - model input
  - `Reload` and `Save` actions wired to `vibefi-code` settings IPC methods
- Added startup settings load (`code_getApiKeys` + `code_getLlmConfig`) with loading/saving state handling.
- Re-validated:
  - `cargo check` passes in `client/`
  - `bun run build` passes in `client/internal-ui/`

### Progress Update 15 (Stage-5 streaming chat foundation)

- Added a dedicated Code chat LLM module set under `internal-ui/src/code/chat/llm/`:
  - `provider.ts` (provider abstraction and shared chat message contract)
  - `claude.ts` (Anthropic streaming via Messages API)
  - `openai.ts` (OpenAI streaming via Responses API)
  - `sse.ts` (shared SSE stream parser)
  - `system.ts` (system prompt construction from project/file/open-buffer context)
- Replaced chat placeholder behavior in `internal-ui/src/code.tsx` with actual message flow:
  - message history state (`user`/`assistant`)
  - textarea input + send action (`Enter` to send, `Shift+Enter` newline)
  - streaming assistant updates, cancel/stop support, clear-chat action
  - provider/model/API key settings integrated with existing saved Code settings
  - inline chat error surfacing
- Online verification pass completed (via sub-agent) before implementation:
  - validated current official package/API guidance for Anthropic and OpenAI
  - kept browser-compatible streaming implementation in internal UI with explicit provider-specific request handling
- Re-validated:
  - `cargo check` passes in `client/`
  - `bun run build` passes in `client/internal-ui/`

## Remaining Implementation TODOs (Handoff)

Date: 2026-02-17

The following major items remain after Progress Update 15:

1. **Stage 6 tool-calling loop**
- Add LLM tool schemas and execution loop (`write_file`, `delete_file`) in the chat provider flow.
- Apply tool edits via existing Code IPC methods.
- Keep open editor buffers and file tree synchronized after tool-applied writes/deletes.
- Track per-turn `ChangeSet` data for diff rendering.

2. **Diff tab implementation**
- Add Diff tab type to editor tab model.
- Compute/render unified diffs for the latest LLM turn’s changeset.
- Wire a chat action to open/focus the Diff tab (e.g. “View Diff”).

3. **Chat robustness and UX**
- Improve SSE/provider error parsing and edge-case handling.
- Refine retry/cancel behavior and streaming state transitions.
- Optionally add markdown rendering for assistant messages.

4. **File tree UX parity**
- Add context menu-style actions from tree nodes (rename/delete/new file/new folder).
- Improve expand/collapse defaults and active-file reveal consistency.

5. **Validation integration (Stage 8 core)**
- Implement `code_validateProject` on Rust side.
- Trigger validation on save/write and surface `[lint]` events in console.
- Expand clickable file:line parsing coverage for lint/build/runtime output.

6. **Settings/security polish**
- Decide/enforce production-safe key handling strategy (proxy/ephemeral tokens).
- Keep direct-browser API-key usage explicit as dev/test path only.

7. **Fork flow (Stage 7)**
- Implement tabbar fork button and source-copy integration to Code workspace.
- Auto-open forked project in Code tab and start Code workflow.

8. **Code UI refactor and tests**
- Split `internal-ui/src/code.tsx` into modular components/files.
- Add focused tests for SSE parsing, chat stream glue, and diff formatting.
### Progress Update 16 (Stage-6 tool loop foundation + tool execution)

- Added Stage-6 tool schema/types in `internal-ui/src/code/chat/llm/tools.ts`:
  - `write_file` and `delete_file` schemas for both Claude and OpenAI payload shapes
  - shared tool-call parsing and validation helpers
  - shared tool execution result contract
- Upgraded chat provider abstraction in `internal-ui/src/code/chat/llm/provider.ts`:
  - `sendChatStream` now supports tool callbacks (`onToolCall`, `onToolResult`) and bounded multi-round loops (`maxToolRounds`)
  - provider calls now return per-turn tool execution results
- Reworked provider implementations to include tool-calling loops:
  - `internal-ui/src/code/chat/llm/claude.ts` now uses non-streaming Claude Messages rounds with `tools`, parses `tool_use`, and sends `tool_result` continuation messages
  - `internal-ui/src/code/chat/llm/openai.ts` now uses non-streaming `chat/completions` rounds with `tools`, parses `tool_calls`, and sends `tool` continuation messages
  - improved provider-side HTTP/API error extraction for clearer chat failures
- Added in-chat tool card UI component:
  - `internal-ui/src/code/chat/ToolCallCard.tsx`
- Integrated tool execution into `internal-ui/src/code.tsx`:
  - executes `write_file`/`delete_file` tool calls via existing `vibefi-code` IPC (`code_writeFile`, `code_deleteFile`)
  - snapshots file content before writes/deletes and tracks per-turn `FileChange[]`
  - updates open file tabs after tool writes and closes tabs on tool deletes
  - refreshes file tree after tool-run turns
  - renders per-tool call cards and applied-change summary inside assistant messages
  - added retry action on chat error using the last sent prompt

Re-validated after this slice:
- `cargo check` passes in `client/` (warnings only)
- `bun run build` passes in `client/internal-ui/`

### Progress Update 23 (IDE layout + console placement + code.tsx split)

Date: 2026-02-19

- Refactored the Code UI to a more IDE-like layout in the internal UI:
  - top workspace bar with mode switch
  - left sidebar panels (`Projects`, `Files`, `Dev Server`, `Console`)
  - center editor area (when enabled)
  - right live preview area
  - bottom chat dock
- Added two workspace modes while preserving existing functionality:
  - `LLM + Preview`
  - `LLM + Code + Preview`
- Fixed console duplication:
  - sidebar `Console` panel is shown only in `LLM + Preview`
  - editor console tab remains in `LLM + Code + Preview`
  - switching out of preview-only mode auto-exits sidebar console selection
- Split `internal-ui/src/code.tsx` into smaller modules for maintainability:
  - `internal-ui/src/code.tsx` (entrypoint only)
  - `internal-ui/src/code/App.tsx`
  - `internal-ui/src/code/styles.ts`
  - `internal-ui/src/code/types.ts`
  - `internal-ui/src/code/constants.ts`
  - `internal-ui/src/code/utils.ts`
  - `internal-ui/src/code/CodeEditor.tsx`
  - `internal-ui/src/code/ChatMessageContent.tsx`

Re-validated after this slice:
- `bun run build` passes in `client/internal-ui/`
- `bunx tsc --noEmit` passes in `client/internal-ui/`

### Progress Update 17 (Diff tab + View Diff wiring)

- Added diff computation utilities for latest-turn changes:
  - `internal-ui/src/code/editor/diff.ts`
  - unified diff generation across created/modified/deleted files from `FileChange[]`
- Added dedicated Diff viewer component:
  - `internal-ui/src/code/editor/DiffViewer.tsx`
  - read-only diff rendering with per-line styling for file headers, hunks, additions, and removals
- Extended editor tab model in `internal-ui/src/code.tsx`:
  - new `DiffTab` kind (`code-diff`) integrated into `EditorTab` union
  - added open/update helper for Diff tab content
  - Diff tab is closeable (like file tabs), Console remains fixed
- Wired chat → Diff behavior:
  - after tool-applied turns with changes, builds unified diff and auto-opens/focuses Diff tab
  - assistant summary now shows `[Applied N file changes]` with `View Diff` action
  - `View Diff` focuses latest-turn diff content
- Added local styling for Diff panel and summary action controls.

Re-validated after this slice:
- `cargo check` passes in `client/` (warnings only)
- `bun run build` passes in `client/internal-ui/`

### Progress Update 18 (chat UX hardening + assistant markdown)

- Added assistant markdown rendering in chat UI:
  - integrated `react-markdown` + `remark-gfm`
  - assistant messages now render markdown/code blocks instead of plain text
  - added chat markdown styling for lists, inline code, and fenced code blocks
- Hardened chat cancellation/retry behavior in `internal-ui/src/code.tsx`:
  - keeps explicit retry path (`Retry`) for last prompt on chat errors
  - abort/cancel now cleans up empty placeholder assistant messages for cleaner history
  - preserves cancellation feedback (`Chat request canceled.`) while avoiding blank responses
- Continued provider robustness improvements already introduced in Stage-6 slice:
  - provider HTTP error body extraction for Claude/OpenAI failures
  - bounded tool-loop rounds to avoid runaway tool-calling sessions
- Added dependencies in `internal-ui/package.json`:
  - `react-markdown`
  - `remark-gfm`

Re-validated after this slice:
- `cargo check` passes in `client/` (warnings only)
- `bun run build` passes in `client/internal-ui/`

### Progress Update 19 (Stage-7 fork flow: tabbar action + backend + Code auto-open)

- Added Stage-7 fork action wiring across tabbar, IPC routing, Rust code provider, and Code UI auto-open flow.

Rust-side changes:
- Extended tab metadata and state for fork support:
  - `src/webview_manager.rs`
    - `AppWebViewEntry` now tracks `source_dir: Option<PathBuf>`
    - tab payload now includes `forkable` (true only for Standard tabs with source + when Code tab exists)
- Extended bundle/tab launch metadata to preserve source origins:
  - `src/bundle.rs`: `BundleConfig` now includes `source_dir`
  - `src/main.rs`: `resolve_bundle` / `resolve_studio_bundle` now persist source dir in `BundleConfig`
  - `src/main.rs`: initial tab creation now sets `source_dir` on `AppWebViewEntry`
  - `src/state.rs`: `TabAction::OpenApp` now carries `source_dir: Option<PathBuf>`
  - `src/registry.rs`: launcher `launch_dapp` now passes inferred source dir into `TabAction::OpenApp`
  - `src/events/user_event.rs`: `open_app_tab` now accepts and stores source dir (with fallback inference from dist)
- Added tabbar method to focus Code tab:
  - `src/ipc_contract.rs`: new `TabbarMethod::SwitchToCodeTab` (`switchToCodeTab`)
  - `src/events/user_event.rs`: handler switches to existing Code tab when requested
- Added tabbar -> vibefi-code IPC forwarding path:
  - `src/events/user_event.rs`: tab-bar IPC handler now forwards `vibefi-code` requests to `code::router` and responds on the tabbar webview
- Implemented `code_forkDapp` backend method:
  - `src/code/router.rs`
    - new `code_forkDapp` params parsing (`webviewId`, optional `name`)
    - validates target tab exists and is Standard
    - validates source availability
    - calls project fork routine and sets active project
    - emits `codeForkComplete` provider event to Code webview with `projectPath`
  - `src/ipc/router.rs`: updated `handle_code_ipc` call signature to include `WebViewManager`
- Implemented source-copy fork routine with collision handling:
  - `src/code/project.rs`: new `fork_project_from_source(...)`
    - copies source tree into `<workspace>/<name>-fork`, `<name>-fork-2`, ...
    - excludes `node_modules`, `.vibefi`, `dist`
    - skips symlinks
    - validates resulting project root (`package.json`, `manifest.json`)

Internal UI changes:
- Tabbar fork UX:
  - `internal-ui/src/ipc/contracts.ts`: `Tab` now supports `forkable?: boolean`
  - `internal-ui/src/tabbar.tsx`
    - renders Fork button on `forkable` tabs
    - calls `vibefi-code` `code_forkDapp` with `{ webviewId }`
    - tracks per-tab pending fork state to prevent duplicate requests
    - on success, sends `switchToCodeTab` to tabbar provider
- Code tab auto-open on fork completion:
  - `internal-ui/src/code.tsx`
    - handles `codeForkComplete` provider event
    - auto-opens returned `projectPath` via existing `openProject(...)` flow

Re-validated after this slice:
- `cargo check` passes in `client/` (warnings only)
- `bun run build` passes in `client/internal-ui/`

### Progress Update 20 (Code-tab crash fix + Stage-8 validator core)

- Fixed the Code tab white-screen runtime crash (`ReferenceError: Cannot access uninitialized variable`):
  - `internal-ui/src/code.tsx`
    - moved `quickOpenFiles` `useMemo(...)` above the `useEffect` that depends on it.
    - this resolves the TDZ access path that occurred during initial render/hook registration.

Rust-side Stage-8 core implementation:
- Added new validator module:
  - `src/code/validator.rs`
    - added `ValidationError` + `ValidationSeverity` models for IPC serialization
    - implemented `validate_project(...)` checks for:
      - directory file-type rules (`src/`, `abis/`, `assets/`)
      - `package.json` dependency allowlist audit
      - manifest schema validation (`capabilities.ipfs.allow` shape/values)
      - forbidden sink scanning (`eval(`, `new Function(`, `innerHTML`, `dangerouslySetInnerHTML`)
    - added `is_valid(...)` helper for `{ valid, errors }` responses
- Wired validator into Code router:
  - `src/code/router.rs`
    - added `code_validateProject` IPC method returning `{ valid, errors }`
    - added validation-on-mutation lint emission for `code_writeFile`, `code_deleteFile`, `code_createDir`
    - lint output now emits as `codeConsoleOutput` with `source: "lint"`
    - lint lines include file/line-aware text for clickable navigation
- Registered validator module export:
  - `src/code/mod.rs` now includes `pub mod validator;`

Internal UI Stage-8 polish slice:
- Expanded console path parsing and normalization coverage:
  - `internal-ui/src/code.tsx`
    - `parseConsolePathMatch(...)` now handles additional patterns:
      - localhost URL stack traces (`http://localhost:.../file.tsx:line:col`)
      - Windows absolute paths (`C:\...\file.ts:line` and `(...line,col)`)
      - Unix absolute paths (`/path/file.tsx:line:col`)
      - relative `file(line,col)` forms in addition to `file:line`
    - added `normalizeConsolePathForProject(...)` to map URL/absolute paths to project-relative file paths when possible
    - `openFileAtLocation(...)` now normalizes matched paths before opening/jumping

Re-validated after this slice:
- `cargo check` passes in `client/` (warnings only)
- `bun run build` passes in `client/internal-ui/`

### Progress Update 21 (chat provider/model mismatch fix)

- Fixed LLM chat provider/model mismatch causing OpenAI sessions to fail with Claude-model errors.

Internal UI changes:
- `internal-ui/src/code.tsx`
  - added provider normalization helper (`normalizeChatProvider`) to map aliases like `chatgpt`/`gpt` to `openai`.
  - added model normalization helper (`normalizeModelForProvider`) to prevent cross-provider model leakage (e.g. Claude model on OpenAI provider).
  - tightened provider state typing to `ChatProvider` (`"claude" | "openai"`).
  - settings load path now normalizes both provider and model before storing in UI state.
  - settings save now persists a normalized provider-compatible model.
  - send path now always resolves a provider-compatible model before request dispatch and updates the visible model field when normalized.
  - provider selector change now auto-normalizes the model to a compatible default when switching providers.

Effect:
- Switching to OpenAI/ChatGPT no longer sends stale Claude model identifiers.
- Entering `chatgpt` as a model alias now resolves to the OpenAI default model (`gpt-4o`) instead of causing provider/model mismatch errors.

Re-validated after this slice:
- `cargo check` passes in `client/` (warnings only)
- `bun run build` passes in `client/internal-ui/`

### Progress Update 22 (dev-server subprocess cleanup on shutdown)

- Hardened Code dev-server shutdown to kill full subprocess trees (not only the `bun` parent PID), addressing dangling `vite/node` processes.

Rust-side changes:
- `src/code/dev_server.rs`
  - dev server is now spawned in its own process group on Unix (`CommandExt::process_group(0)`).
  - added process-tree stop path used by both normal stop and app shutdown:
    - graceful terminate phase (SIGTERM to process group on Unix)
    - bounded wait
    - force-kill phase (SIGKILL to process group on Unix)
  - added `stop_dev_server_for_shutdown(...)` helper for app teardown.
  - retained platform-safe fallback behavior (`child.kill()`), plus Windows taskkill hooks under cfg.
- `src/state.rs`
  - extended `RunningCodeDevServer` with `uses_process_group: bool` so stop logic knows whether group signaling is safe.
- `src/main.rs`
  - added `Event::LoopDestroyed` cleanup hook to always stop the Code dev server during app shutdown.

Effect:
- Closing the client now performs explicit dev-server process-tree cleanup and prevents orphaned `node ... vite dev` children from persisting after app exit.

Re-validated after this slice:
- `cargo check` passes in `client/` (warnings only)
- `bun run build` passes in `client/internal-ui/`
