# VibeFi Code — Implementation Spec v1

## Overview

VibeFi Code is an integrated development environment tab within the VibeFi client that lets users vibe-code dapps using LLM assistants (Claude, OpenAI). Users can fork existing running dapps or create new ones, edit source in a CodeMirror 6 editor, chat with an LLM that can read and write project files, and see live previews via a Vite dev server — all without leaving the client.

---

## 1. Naming & Identity

- **User-facing name**: "VibeFi Code"
- **Rust enum variant**: Rename `AppWebViewKind::Studio` → `AppWebViewKind::Code`
- **Tab label**: "Code" (short form in tab bar)
- **IPC provider ID**: `vibefi-code`
- **Workspace root**: `~/.local/share/VibeFi/code/` (Linux), `~/Library/Application Support/VibeFi/code/` (macOS), `%APPDATA%/VibeFi/code/` (Windows)

---

## 2. Layout

```
┌──────────────────────────────────────────────────────────────┐
│  [Launcher]  [Code]  [Aave V3]  [Uniswap ⑂]               │
├─────────┬────────────────────────────┬───────────────────────┤
│         │ [App.tsx ×] [hooks.ts ×]   │                       │
│  File   │ [Console] [Diff]           │                       │
│  Tree   │                            │   Preview             │
│         │  ┌──────────────────────┐  │   (iframe →           │
│         │  │  Editor / Console /  │  │    localhost:PORT)     │
│         │  │  Diff content        │  │                       │
│         │  │                      │  │                       │
│         │  └──────────────────────┘  │                       │
│         ├────────────────────────────┴───────────────────────┤
│         │  💬 LLM Chat                                [▾ ▴]  │
│         │  ┌─────────────────────────────────────────────┐   │
│         │  │ User: Add a supply position table           │   │
│         │  │ Assistant: I'll add a table component...     │   │
│         │  │ [Applied 3 file changes] [View Diff]        │   │
│         │  └─────────────────────────────────────────────┘   │
│         │  [Type a message...                     ] [Send]   │
└─────────┴────────────────────────────────────────────────────┘
```

### Panel Structure

| Panel | Position | Resizable | Description |
|---|---|---|---|
| **File Tree** | Left sidebar | Width-resizable | Project file explorer |
| **Editor Tabs** | Center-top | — | Tabbed pane for open files, console, and diff |
| **Preview** | Right | Width-resizable | iframe pointing at Vite dev server |
| **Chat** | Bottom | Height-resizable, collapsible | Multi-turn LLM conversation |

### Editor Tabs (center pane)

The editor area is a tabbed container. Tabs come in three types:

1. **File tabs** — CodeMirror 6 instance for an open file. Closeable (`×`). Dirty indicator (dot) when unsaved.
2. **Console tab** — Always present, not closeable. Shows:
   - Vite dev server stdout/stderr (build errors, HMR status)
   - Constraint validation errors
   - Runtime errors forwarded from the preview iframe (via `postMessage`)
3. **Diff tab** — Opens after LLM applies changes. Shows a unified diff of all files modified in the last LLM turn. Read-only. Closeable.

---

## 3. Content Security Policy

The VibeFi Code tab uses a **relaxed CSP** distinct from standard dapp tabs.

### Code Tab CSP

```
default-src 'self' app:;
script-src 'self' 'unsafe-inline' app:;
style-src 'self' 'unsafe-inline' app:;
connect-src
  https://api.anthropic.com
  https://api.openai.com;
frame-src http://localhost:*;
img-src 'self' data: app: http://localhost:*;
font-src 'self' app: data:;
object-src 'none';
base-uri 'none';
form-action 'none';
```

### Key Differences from Standard Dapp CSP

| Directive | Standard Tab | VibeFi Code Tab | Reason |
|---|---|---|---|
| `connect-src` | `'none'` | Claude + OpenAI origins | LLM API calls from JS |
| `frame-src` | `'none'` | `http://localhost:*` | Preview iframe to Vite dev server |
| `img-src` | `'self' data: app:` | + `http://localhost:*` | Images from preview |
| `script-src` | `'self' app:` | + `'unsafe-inline'` | CodeMirror and inline scripts |

### Future: Ollama Support

When Ollama is added, append `http://localhost:11434` to `connect-src`.

### Implementation

In `src/webview.rs`, branch on the `AppWebViewKind` when constructing the CSP meta tag:

```rust
fn csp_for_kind(kind: &AppWebViewKind) -> &'static str {
    match kind {
        AppWebViewKind::Code => CODE_CSP,
        _ => STANDARD_CSP,
    }
}
```

---

## 4. CodeMirror 6 Editor

### Dependencies

```json
{
  "@codemirror/view": "^6.x",
  "@codemirror/state": "^6.x",
  "@codemirror/commands": "^6.x",
  "@codemirror/language": "^6.x",
  "@codemirror/lang-javascript": "^6.x",
  "@codemirror/lang-json": "^6.x",
  "@codemirror/lang-html": "^6.x",
  "@codemirror/lang-css": "^6.x",
  "@codemirror/search": "^6.x",
  "@codemirror/theme-one-dark": "^6.x"
}
```

### Configuration

Minimal setup — the LLM is the primary authoring tool, the editor is for review and small tweaks:

- Syntax highlighting (language auto-detected from file extension)
- Line numbers
- Active line highlight
- Bracket matching
- Basic keybindings (undo/redo, indent, comment toggle)
- Search/replace (`Ctrl+F` / `Cmd+F`)
- Dark theme (one-dark) to match the client aesthetic
- Read-only mode toggle (used for diff view)
- No LSP, no autocomplete, no minimap

### Language Detection

```typescript
function langFromPath(path: string): LanguageSupport | null {
  if (/\.tsx?$/.test(path)) return javascript({ typescript: true, jsx: true });
  if (/\.jsx?$/.test(path)) return javascript({ jsx: true });
  if (/\.json$/.test(path)) return json();
  if (/\.html?$/.test(path)) return html();
  if (/\.css$/.test(path))  return css();
  return null;
}
```

### Save Behavior

- `Ctrl+S` / `Cmd+S` triggers save: sends `studio_writeFile` IPC, clears dirty indicator.
- Auto-save on focus loss (switching tabs, clicking preview, switching to chat) with a 1-second debounce.
- Vite HMR picks up the disk write automatically — no explicit rebuild step.

---

## 5. Live Preview via Vite Dev Server

### Lifecycle

1. **Project opened** → Rust runs `bun install` (if `node_modules/` missing) then spawns `bun x vite dev --port <PORT> --host localhost` as a child process.
2. **Port allocation** → Start at port `5199`, increment until an open port is found. Return the port to the JS side in the `studio_startDevServer` IPC response.
3. **Preview iframe** → `src` set to `http://localhost:<PORT>`.
4. **File saved** → Written to disk via IPC → Vite's file watcher detects change → HMR update pushed to iframe via WebSocket (Vite handles this natively).
5. **Project closed / client quit** → Rust sends `SIGTERM` to the child process, waits briefly, then `SIGKILL` if needed.

### Process Management (Rust side)

```rust
struct DevServer {
    child: std::process::Child,
    port: u16,
    project_path: PathBuf,
}
```

- Store in `AppState` behind a `Mutex<Option<DevServer>>`.
- Only one dev server at a time (v1 — single project open).
- Forward stdout/stderr to the Code tab's Console via IPC events:
  ```
  UserEvent::CodeConsoleOutput { line: String }
  ```
- Detect "ready" by watching stdout for Vite's `Local: http://localhost:<PORT>` line → then tell the JS side the preview is ready.

### Vite Config Injection

When creating or forking a project, ensure a working `vite.config.ts` exists. The scaffold template includes one that:

- Sets `server.port` to the allocated port
- Sets `server.strictPort = false` (allows fallback)
- Sets `server.hmr.host = 'localhost'`
- Defines `RPC_URL` as a Vite env variable

```typescript
// vite.config.ts (scaffold)
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  define: {
    "globalThis.RPC_URL": JSON.stringify(process.env.RPC_URL ?? ""),
  },
  server: {
    strictPort: false,
    hmr: { host: "localhost" },
  },
});
```

### Runtime Errors from Preview

The preview iframe can forward errors to the parent via `postMessage`:

```javascript
// Injected into the preview via Vite plugin or index.html
window.addEventListener("error", (e) => {
  window.parent.postMessage({ type: "vibefi-code-error", message: e.message, stack: e.stack }, "*");
});
window.addEventListener("unhandledrejection", (e) => {
  window.parent.postMessage({ type: "vibefi-code-error", message: String(e.reason) }, "*");
});
```

The Code tab listens for these and appends them to the Console tab with a red error style.

---

## 6. LLM Chat Integration

### Provider Configuration

Users configure API keys in a settings panel accessible from the chat pane (gear icon). Stored via `vibefi-code` IPC methods that persist to the settings file.

```typescript
interface LlmConfig {
  provider: "claude" | "openai";
  apiKey: string;
  model: string; // e.g. "claude-sonnet-4-5-20250929", "gpt-4o"
}
```

**Default models:**
- Claude: `claude-sonnet-4-5-20250929`
- OpenAI: `gpt-4o`

The user can override the model in settings.

### Multi-Turn Conversation

The chat maintains a full conversation history in React state. Each message sent to the API includes:

1. **System prompt** (see Section 6.2)
2. **Full conversation history** (user + assistant messages)

Context is not truncated in v1. If context limits become an issue, we can add summarization later.

### System Prompt Construction

On each LLM request, the JS side constructs a system prompt:

```
You are VibeFi Code, an AI assistant for building VibeFi dapps.

## Constraints
{contents of constraints.md, embedded at build time}

## Current Project Structure
{output of studio_listFiles — file tree with sizes}

## Open Files
{for each open editor tab: filename + full contents}

## Tools Available
You can use the following tools to modify the project:
- write_file(path, content): Write or create a file
- delete_file(path): Delete a file

Apply changes directly using tools. Do not ask for confirmation — changes are
auto-applied and the user can review diffs afterward.

When writing code, follow the project's existing patterns and the VibeFi
constraints. Only use approved packages. Use window.ethereum for wallet
interactions and window.vibefiIpfs for IPFS reads.
```

### Tool Use for File Edits

The LLM uses **tool calling** (Claude's `tool_use`, OpenAI's `function_calling`) to make file changes. The client defines two tools:

#### `write_file`

```json
{
  "name": "write_file",
  "description": "Create or overwrite a file in the project. Path is relative to project root.",
  "input_schema": {
    "type": "object",
    "properties": {
      "path": { "type": "string", "description": "Relative file path, e.g. src/components/Table.tsx" },
      "content": { "type": "string", "description": "Full file content" }
    },
    "required": ["path", "content"]
  }
}
```

#### `delete_file`

```json
{
  "name": "delete_file",
  "description": "Delete a file from the project.",
  "input_schema": {
    "type": "object",
    "properties": {
      "path": { "type": "string", "description": "Relative file path to delete" }
    },
    "required": ["path"]
  }
}
```

### Auto-Apply Flow

1. User sends a message in the chat.
2. JS sends the request to Claude/OpenAI with tools defined.
3. LLM streams back a response. When a tool call is encountered:
   a. `write_file` → JS calls `studio_writeFile` IPC → file written to disk → Vite HMR updates preview.
   b. `delete_file` → JS calls `studio_deleteFile` IPC → file removed.
   c. Track all changes in a `ChangeSet[]` for the diff view.
4. After all tool calls are processed, tool results are sent back to the LLM to continue its response (standard tool-use loop).
5. When the LLM's turn is complete, if any files were changed:
   a. Open the **Diff tab** showing a unified diff of all changes.
   b. Show a summary in the chat: `[Applied N file changes] [View Diff]`.
   c. If an open file was modified, update the CodeMirror buffer (without triggering a re-save).
6. File tree refreshes automatically after any file change.

### Streaming

Use native `fetch()` with streaming:

```typescript
const response = await fetch("https://api.anthropic.com/v1/messages", {
  method: "POST",
  headers: {
    "Content-Type": "application/json",
    "x-api-key": config.apiKey,
    "anthropic-version": "2023-06-01",
    "anthropic-dangerous-direct-browser-access": "true",
  },
  body: JSON.stringify({ model, system, messages, tools, stream: true }),
});

const reader = response.body!.getReader();
// Process SSE chunks, update chat UI progressively
```

For OpenAI, similar approach with `stream: true` and SSE parsing.

### Chat UI Details

- Messages rendered as markdown (use a lightweight renderer — `marked` or similar, already in the approved stack or bundled separately for internal-ui).
- Code blocks in assistant messages are syntax-highlighted.
- Tool call results shown as collapsible cards: `[write_file: src/components/Table.tsx]` with an expand arrow to see the full content.
- "View Diff" button on the summary card opens/focuses the Diff tab.
- "Clear Chat" button resets conversation history.
- Chat pane is collapsible (drag handle or toggle button) to maximize editor/preview space.

---

## 7. Diff View

### Trigger

The Diff tab opens (or updates) whenever the LLM applies file changes. It shows a **unified diff of all files changed in the most recent LLM turn**.

### Format

```
── src/components/Table.tsx (created) ─────────────
+ import React from "react";
+ export function Table({ data }: { data: any[] }) {
+   return <table>...</table>;
+ }

── src/App.tsx (modified) ─────────────────────────
@@ -5,6 +5,7 @@
  import { Header } from "./components/Header";
+ import { Table } from "./components/Table";

  function App() {
@@ -12,6 +13,7 @@
    return (
      <div>
        <Header />
+       <Table data={positions} />
      </div>
    );
```

### Implementation

- Compute diffs client-side using a lightweight diff library (e.g. `diff` npm package or a minimal inline implementation).
- Before each `write_file` tool call, snapshot the current file content (read from IPC or cache).
- After write, compute unified diff between old and new.
- Render in a read-only CodeMirror instance with red/green line highlighting.

### Undo

v1 does not include undo/revert from the diff view. The user can manually revert by editing or asking the LLM to undo. Git-backed undo is a future enhancement.

---

## 8. Console Tab

### Content Sources

The Console tab aggregates output from multiple sources, each with a distinct prefix/color:

| Source | Prefix | Color | Description |
|---|---|---|---|
| Vite | `[vite]` | Cyan | Dev server stdout (HMR updates, build status) |
| Build Error | `[build]` | Red | Vite compilation errors |
| Runtime | `[runtime]` | Orange | Errors from the preview iframe via `postMessage` |
| Constraint | `[lint]` | Yellow | VibeFi constraint violations |

### Behavior

- Auto-scrolls to bottom on new output (unless the user has scrolled up).
- "Clear" button to reset the console.
- Monospace font, dark background.
- Clickable file paths in error messages → opens the file in an editor tab and jumps to the line.

### Constraint Validation

Run on every file save. Checks:

1. **File type enforcement**: Only `.ts`, `.tsx`, `.css` in `src/`; `.json` in `abis/`; `.webp` in `assets/`.
2. **Package.json audit**: Only approved dependencies present.
3. **Security lint**: Scan for `eval(`, `new Function(`, `innerHTML`, `dangerouslySetInnerHTML` usage patterns.
4. **Manifest validation**: If `manifest.json` exists, validate `capabilities` schema.

Violations appear in the Console tab and optionally as inline editor markers (yellow underlines) in a future version.

---

## 9. File Tree

### Display

- Recursive directory tree with expand/collapse.
- Icons for file types (TS, JSON, CSS, HTML, image).
- File size shown on hover.
- Currently open file highlighted.
- Right-click context menu: Open, Rename, Delete, New File, New Folder.

### IPC

Uses `studio_listFiles` which returns:

```typescript
interface FileEntry {
  name: string;
  path: string;       // relative to project root
  isDir: boolean;
  size?: number;       // bytes, files only
  children?: FileEntry[]; // dirs only
}
```

### Refresh

- Automatically refreshes after any `studio_writeFile` or `studio_deleteFile` call.
- Manual refresh button in the tree header.

---

## 10. Fork Flow

### Entry Point

Each dapp tab in the main tab bar shows a **fork button** (⑂ icon) on the right side of the tab label. Clicking it triggers the fork.

```
  [Launcher]  [Code]  [Aave V3 ⑂]  [Safe Admin ⑂]
```

The fork button is only visible on `Standard` (dapp) tabs.

### Fork Process

1. User clicks ⑂ on a running dapp tab.
2. Client sends `UserEvent::ForkDapp { webview_id }` to Rust.
3. Rust resolves the dapp's **source bundle path**:
   - If loaded via `--bundle`: use the original source directory.
   - If loaded via IPFS registry: use the cached pre-build source (the fetched files before `vite build`).
   - If only compiled `dist/` is available: show a toast "Source not available for this dapp" and abort.
4. Rust copies the source files to `<workspace_root>/<dapp-name>-fork/`:
   - `src/`, `abis/`, `assets/`, `addresses.json`, `manifest.json`, `package.json`, `index.html`, `tsconfig.json`, `vite.config.ts`
   - Exclude `node_modules/`, `.vibefi/`, `dist/`.
5. Rust sends `UserEvent::ForkComplete { project_path }`.
6. JS switches to the Code tab, loads the forked project, starts the dev server.

### Name Collision

If `<dapp-name>-fork/` already exists, append a numeric suffix: `<dapp-name>-fork-2/`, etc.

---

## 11. New Project Scaffold

### Template

`studio_createProject` scaffolds a minimal dapp:

```
<project-name>/
├── src/
│   ├── App.tsx          # Minimal React app with wallet connection
│   ├── App.css          # Basic styles
│   └── main.tsx         # React DOM render entry
├── abis/                # Empty directory
├── assets/              # Empty directory
├── addresses.json       # {}
├── manifest.json        # { "name": "<project-name>", "version": "0.1.0" }
├── package.json         # Approved deps only
├── index.html           # Standard Vite entry
├── tsconfig.json        # Strict TS config
└── vite.config.ts       # Standard Vite + React config
```

### Scaffold App.tsx

```tsx
import { useState, useEffect } from "react";
import "./App.css";

function App() {
  const [account, setAccount] = useState<string | null>(null);

  async function connect() {
    if (!window.ethereum) return;
    const accounts = await window.ethereum.request({
      method: "eth_requestAccounts",
    });
    setAccount(accounts[0] ?? null);
  }

  return (
    <div className="app">
      <h1>My VibeFi Dapp</h1>
      {account ? (
        <p>Connected: {account.slice(0, 6)}...{account.slice(-4)}</p>
      ) : (
        <button onClick={connect}>Connect Wallet</button>
      )}
    </div>
  );
}

export default App;
```

---

## 12. IPC Contract

### Provider: `vibefi-code`

All methods use the existing `IpcRequest` / `IpcResponse` pattern via `window.ipc.postMessage`.

#### File Operations

| Method | Params | Response | Side Effects |
|---|---|---|---|
| `code_listFiles` | `{ projectPath: string }` | `{ files: FileEntry[] }` | None |
| `code_readFile` | `{ projectPath: string, filePath: string }` | `{ content: string }` | None |
| `code_writeFile` | `{ projectPath: string, filePath: string, content: string }` | `{ ok: true }` | Disk write → Vite HMR |
| `code_deleteFile` | `{ projectPath: string, filePath: string }` | `{ ok: true }` | Disk delete |
| `code_createDir` | `{ projectPath: string, dirPath: string }` | `{ ok: true }` | `mkdir -p` equivalent |

#### Project Management

| Method | Params | Response | Side Effects |
|---|---|---|---|
| `code_createProject` | `{ name: string }` | `{ projectPath: string }` | Scaffold on disk |
| `code_listProjects` | `{}` | `{ projects: { name, path, lastModified }[] }` | None |
| `code_openProject` | `{ path?: string }` | `{ projectPath: string, files: FileEntry[] }` | Native dir picker if no path |
| `code_forkDapp` | `{ webviewId: string, name?: string }` | `{ projectPath: string }` | Copy source to workspace |

#### Dev Server

| Method | Params | Response | Side Effects |
|---|---|---|---|
| `code_startDevServer` | `{ projectPath: string }` | `{ port: number }` | Spawns `bun dev` |
| `code_stopDevServer` | `{}` | `{ ok: true }` | Kills child process |
| `code_devServerStatus` | `{}` | `{ running: bool, port?: number }` | None |

#### Settings

| Method | Params | Response | Side Effects |
|---|---|---|---|
| `code_getApiKeys` | `{}` | `{ claude?: string, openai?: string }` | None |
| `code_setApiKeys` | `{ claude?: string, openai?: string }` | `{ ok: true }` | Persist to settings |
| `code_getLlmConfig` | `{}` | `{ provider, model }` | None |
| `code_setLlmConfig` | `{ provider, model }` | `{ ok: true }` | Persist to settings |

#### Validation

| Method | Params | Response |
|---|---|---|
| `code_validateProject` | `{ projectPath: string }` | `{ valid: bool, errors: ValidationError[] }` |

```typescript
interface ValidationError {
  severity: "error" | "warning";
  file?: string;
  line?: number;
  message: string;
  rule: string; // e.g. "disallowed-package", "forbidden-sink", "invalid-file-type"
}
```

### IPC Events (Rust → JS, push-based)

| Event | Payload | Description |
|---|---|---|
| `codeConsoleOutput` | `{ source: string, line: string }` | Dev server stdout/stderr |
| `codeDevServerReady` | `{ port: number }` | Dev server is listening |
| `codeDevServerExit` | `{ code: number }` | Dev server process exited |
| `codeFileChanged` | `{ path: string, kind: "create" \| "modify" \| "delete" }` | External file change detected (future: file watcher) |

---

## 13. Rust-Side Implementation

### New Modules

```
src/
├── code/
│   ├── mod.rs            # Public interface, re-exports
│   ├── router.rs         # IPC method dispatch for vibefi-code provider
│   ├── filesystem.rs     # File operations (list, read, write, delete, mkdir)
│   ├── project.rs        # Create, fork, list projects; scaffold template
│   ├── dev_server.rs     # Spawn/kill bun dev, port allocation, stdout forwarding
│   └── validator.rs      # Constraint checking
```

### State Additions

```rust
// In AppState or a new CodeState
pub struct CodeState {
    /// Currently active project path
    pub active_project: Option<PathBuf>,
    /// Running dev server process
    pub dev_server: Option<DevServer>,
    /// Workspace root directory
    pub workspace_root: PathBuf,
}

pub struct DevServer {
    pub child: std::process::Child,
    pub port: u16,
    pub project_path: PathBuf,
}
```

### IPC Router Integration

In `src/ipc/router.rs`, add a branch for the `vibefi-code` provider:

```rust
Some("vibefi-code") => code::router::handle_code_ipc(req, state, proxy),
```

### Dev Server Process Management

```rust
// src/code/dev_server.rs

pub fn start_dev_server(
    project_path: &Path,
    proxy: EventLoopProxy<UserEvent>,
) -> Result<DevServer> {
    let port = find_available_port(5199)?;

    let child = Command::new("bun")
        .args(["x", "vite", "dev", "--port", &port.to_string(), "--host", "localhost"])
        .current_dir(project_path)
        .env("RPC_URL", /* from active config */)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // Spawn thread to read stdout and forward to JS
    let stdout = child.stdout.take().unwrap();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            if let Ok(line) = line {
                if line.contains("Local:") && line.contains("localhost") {
                    let _ = proxy.send_event(UserEvent::CodeDevServerReady { port });
                }
                let _ = proxy.send_event(UserEvent::CodeConsoleOutput {
                    source: "vite".into(),
                    line,
                });
            }
        }
    });

    Ok(DevServer { child, port, project_path: project_path.to_owned() })
}
```

### Filesystem Security

All file operations in `filesystem.rs` MUST validate that the resolved path is within the project directory to prevent path traversal:

```rust
fn validate_path(project_root: &Path, relative_path: &str) -> Result<PathBuf> {
    let resolved = project_root.join(relative_path).canonicalize()?;
    if !resolved.starts_with(project_root.canonicalize()?) {
        return Err(anyhow!("path traversal attempt: {}", relative_path));
    }
    Ok(resolved)
}
```

Additionally, enforce VibeFi file type constraints:
- Only allow writing files with approved extensions (`.ts`, `.tsx`, `.css`, `.json`, `.html`, `.webp`)
- Block writes to `node_modules/`, `.vibefi/`, or any dotfile directories

### Bun Install

Before starting the dev server, check if `node_modules/` exists. If not, run `bun install` first and stream its output to the console:

```rust
if !project_path.join("node_modules").exists() {
    let status = Command::new("bun")
        .arg("install")
        .current_dir(project_path)
        .status()?;
    if !status.success() {
        return Err(anyhow!("bun install failed"));
    }
}
```

---

## 14. Internal-UI Implementation

### New Entry Point

Add `internal-ui/src/code.tsx` as the entry for the VibeFi Code tab, similar to how `launcher.tsx`, `settings.tsx`, etc. are separate entry points.

### Key React Components

```
internal-ui/src/
├── code.tsx                    # Entry point, root layout
├── code/
│   ├── layout/
│   │   ├── CodeLayout.tsx      # Main split-pane layout
│   │   ├── Sidebar.tsx         # File tree container
│   │   ├── EditorPane.tsx      # Tabbed editor area
│   │   └── ChatPane.tsx        # Bottom chat area
│   ├── editor/
│   │   ├── CodeEditor.tsx      # CodeMirror 6 wrapper component
│   │   ├── DiffViewer.tsx      # Diff tab content
│   │   ├── Console.tsx         # Console tab content
│   │   └── EditorTabs.tsx      # Tab bar for editor pane
│   ├── filetree/
│   │   ├── FileTree.tsx        # Recursive tree component
│   │   └── FileIcon.tsx        # File type icons
│   ├── chat/
│   │   ├── Chat.tsx            # Chat container
│   │   ├── MessageList.tsx     # Scrollable message history
│   │   ├── Message.tsx         # Single message (user or assistant)
│   │   ├── ToolCallCard.tsx    # Collapsible file change card
│   │   ├── ChatInput.tsx       # Input textarea + send button
│   │   └── llm/
│   │       ├── provider.ts     # LLM provider abstraction
│   │       ├── claude.ts       # Claude API client (streaming)
│   │       ├── openai.ts       # OpenAI API client (streaming)
│   │       ├── tools.ts        # Tool definitions
│   │       └── system.ts       # System prompt builder
│   ├── preview/
│   │   └── Preview.tsx         # iframe wrapper
│   └── state/
│       ├── project.ts          # Project state (files, active file, dirty flags)
│       ├── chat.ts             # Chat state (messages, streaming status)
│       └── devserver.ts        # Dev server state (port, status)
```

### State Management

Use React context + `useReducer` for each state domain. No external state library needed for v1.

```typescript
// Project state
interface ProjectState {
  projectPath: string | null;
  files: FileEntry[];
  openTabs: EditorTab[];
  activeTabId: string;
  dirtyFiles: Set<string>; // paths with unsaved changes
}

// Chat state
interface ChatState {
  messages: ChatMessage[];
  isStreaming: boolean;
  lastChangeSet: FileChange[]; // for diff view
}

// Dev server state
interface DevServerState {
  status: "stopped" | "starting" | "running" | "error";
  port: number | null;
  consoleLines: ConsoleLine[];
}
```

---

## 15. Startup Flow

### When the Code Tab is First Opened

1. Code tab loads `code.html` entry point.
2. JS checks for API key configuration. If none set, show a setup prompt:
   ```
   Welcome to VibeFi Code!
   To get started, configure your LLM provider:
   [Claude API Key: ________]  [OpenAI API Key: ________]
   [Save & Continue]
   ```
3. JS calls `code_listProjects` to show existing projects.
4. User picks: **Open Existing**, **Create New**, or arrives via **Fork** (auto-loaded).
5. On project load:
   a. File tree populates.
   b. `bun install` runs if needed (progress shown in console).
   c. Dev server starts.
   d. Preview iframe loads when `codeDevServerReady` event arrives.
   e. `src/App.tsx` (or `index.html`) opens in the editor.

### When Arriving via Fork

Steps 1-2 same as above. Step 4 is skipped — the forked project is auto-loaded. Steps 5a-e proceed automatically.

---

## 16. Security Considerations

### LLM API Keys

- Stored via `code_setApiKeys` IPC, persisted in the VibeFi settings file on disk.
- Never sent to any endpoint other than the configured LLM provider.
- The relaxed CSP for the Code tab only allows `connect-src` to specific API origins, preventing exfiltration to arbitrary endpoints.
- Keys are not exposed to the preview iframe or dapp tabs.

### Preview Iframe Isolation

- The preview iframe runs on `http://localhost:<PORT>`, a different origin from `app://` — same-origin policy prevents the preview from accessing the Code tab's DOM or JS context.
- Communication between the Code tab and preview is strictly via `postMessage` (for error forwarding only).
- The preview iframe has no access to IPC, API keys, or Rust backend — it's a standard Vite dev server page.

### Filesystem Sandboxing

- All file operations are constrained to the project directory via path traversal checks.
- The Rust side enforces allowed file extensions.
- No shell execution from the JS side — all commands go through specific IPC methods.

### LLM-Generated Code

- The constraint validator runs on every save to catch violations.
- The LLM system prompt includes the constraints, making violations less likely.
- Build errors from Vite surface immediately in the console.
- Users are responsible for reviewing LLM-generated code (the diff view aids this).

---

## 17. Implementation Stages

Each stage is a self-contained unit of work that can be assigned to a separate coding session/model instance. Stages build on each other sequentially — each stage's deliverables are prerequisites for the next. Every stage should end with the feature being testable in isolation.

---

### Stage 1: Rust Foundation — Rename, IPC Skeleton, Filesystem

**Goal**: The Code tab exists, loads an empty page, and can read/write files on disk through IPC.

**Scope**:
- Rename `AppWebViewKind::Studio` → `AppWebViewKind::Code` across all Rust source (enum variant, match arms, comments, CLI flag `--studio-bundle` → `--code-bundle`).
- Create `src/code/` module directory with `mod.rs`, `router.rs`, `filesystem.rs`, `project.rs`.
- Implement `CodeState` struct and add it to `AppState`.
- Implement filesystem IPC methods in `router.rs`:
  - `code_listFiles` — recursive directory listing, returns `FileEntry[]` tree.
  - `code_readFile` — read file content as UTF-8 string.
  - `code_writeFile` — write content to file, create parent dirs if needed.
  - `code_deleteFile` — remove a file.
  - `code_createDir` — `mkdir -p` equivalent.
- Implement path traversal guard in `filesystem.rs` (`validate_path`).
- Implement file extension allowlist enforcement.
- Wire `vibefi-code` provider into `src/ipc/router.rs` dispatch.
- Add the relaxed CSP for `AppWebViewKind::Code` in `src/webview.rs` (branching `csp_for_kind`).
- Create a minimal `internal-ui/src/code.html` and `internal-ui/src/code.tsx` entry point that renders "VibeFi Code" and demonstrates a round-trip IPC call (e.g. list files).

**Test**: Launch the client, click the Code tab, see the placeholder page. Open the browser devtools console and verify `code_listFiles` returns data for a test directory.

**Key files touched**:
- `src/webview_manager.rs` (enum rename)
- `src/webview.rs` (CSP branch)
- `src/ipc/router.rs` (new provider)
- `src/code/mod.rs`, `router.rs`, `filesystem.rs` (new)
- `src/state.rs` (CodeState)
- `src/events/user_event.rs` (rename Studio references)
- `src/main.rs` (rename Studio references)
- `src/config/cli.rs` (rename flag)
- `internal-ui/src/code.html`, `internal-ui/src/code.tsx` (new)

**Estimated complexity**: Medium. Mostly plumbing and renaming with one substantive piece (filesystem ops + security).

---

### Stage 2: Project Management — Scaffold, List, Open, Workspace

**Goal**: Users can create new projects from a template, list existing projects, and open them. The workspace directory structure exists.

**Scope**:
- Implement `project.rs`:
  - `code_createProject` — scaffold a new project from the template (all files from Section 11: `App.tsx`, `main.tsx`, `App.css`, `package.json`, `vite.config.ts`, `tsconfig.json`, `index.html`, `manifest.json`, `addresses.json`, empty `abis/` and `assets/` dirs).
  - `code_listProjects` — scan workspace root, return `{ name, path, lastModified }[]`.
  - `code_openProject` — validate a directory is a VibeFi project (has `package.json` + `manifest.json`), return its file tree.
- Determine workspace root per-platform using the `dirs` crate (or existing VibeFi data dir logic).
- Embed scaffold template files as `include_str!()` constants in a `src/code/template/` directory or inline in `project.rs`.
- In the internal-ui, build a **project picker** view that shows on Code tab load:
  - List of existing projects (from `code_listProjects`).
  - "New Project" button → name input → calls `code_createProject` → opens the project.
  - "Open Folder" button → calls `code_openProject` (with native dir picker, if Wry supports it, otherwise a path input).

**Test**: Create a new project "my-test-dapp", see it appear in the workspace directory on disk with all scaffold files. Close and reopen the Code tab, see it listed. Open it successfully.

**Key files touched**:
- `src/code/project.rs` (new)
- `src/code/router.rs` (add project methods)
- `internal-ui/src/code.tsx` and new `code/ProjectPicker.tsx`

**Estimated complexity**: Medium. Template embedding and directory operations. The project picker UI is straightforward.

---

### Stage 3: Dev Server — Spawn, Port Allocation, Console Output

**Goal**: Opening a project automatically starts `bun dev`, streams output to a console, and provides the port number for the preview.

**Scope**:
- Implement `src/code/dev_server.rs`:
  - `find_available_port(base: u16)` — bind-test ports starting from 5199.
  - `start_dev_server(project_path, proxy)` — run `bun install` if needed, then spawn `bun x vite dev`, pipe stdout/stderr.
  - `stop_dev_server(state)` — send `SIGTERM`, wait, `SIGKILL` if needed.
  - Stdout reader thread that emits `UserEvent::CodeConsoleOutput` and detects `UserEvent::CodeDevServerReady`.
- Add `code_startDevServer`, `code_stopDevServer`, `code_devServerStatus` IPC methods to the router.
- Add `UserEvent` variants: `CodeConsoleOutput { source, line }`, `CodeDevServerReady { port }`, `CodeDevServerExit { code }`.
- In the event loop (`main.rs`), handle these events by dispatching to the Code webview via `ui_bridge::dispatch`.
- In `internal-ui`, build a minimal **Console** component:
  - Receives `codeConsoleOutput` events, appends lines with source-colored prefixes.
  - Shows a "Starting dev server..." message, then transitions to showing the port when ready.
  - Auto-scroll behavior.
  - "Clear" button.
- Wire the dev server lifecycle: start on project open, stop on project close / tab teardown / client quit.
- Ensure proper cleanup on client exit (kill child process in a drop guard or shutdown hook).

**Test**: Open a scaffolded project from Stage 2. See `bun install` output in the console, then Vite startup. Verify the dev server is reachable at the reported port via a browser. Close the project, verify the process is killed.

**Key files touched**:
- `src/code/dev_server.rs` (new)
- `src/code/router.rs` (add dev server methods)
- `src/state.rs` (DevServer struct, add to CodeState)
- `src/events/user_event.rs` (new variants)
- `src/main.rs` (handle new events, cleanup on exit)
- `internal-ui/src/code/editor/Console.tsx` (new)

**Estimated complexity**: Medium-High. Process management, async stdout forwarding, and cleanup are the tricky parts.

---

### Stage 4: Editor UI — File Tree, CodeMirror, Tabbed Editor, Preview

**Goal**: The full Code tab layout is functional — file tree, tabbed CodeMirror editor, preview iframe, and console. Users can browse files, open them in editor tabs, edit, save, and see live updates in the preview.

**Scope**:
- Install CodeMirror 6 packages in `internal-ui/package.json`.
- Build the **split-pane layout** (`CodeLayout.tsx`):
  - Left sidebar (file tree), center (editor tabs), right (preview iframe), bottom (chat placeholder).
  - Resizable dividers between panes (use a lightweight split-pane lib or CSS `resize`).
- Build **FileTree** component:
  - Recursive tree from `code_listFiles` response.
  - Expand/collapse directories.
  - Click file → open in editor tab.
  - Right-click context menu: New File, New Folder, Delete.
  - File type icons (simple SVG or CSS-based).
- Build **EditorTabs** component:
  - Tab bar showing open files + always-present Console tab.
  - Active tab indicator, dirty dot, close button.
  - Click to switch, middle-click to close.
- Build **CodeEditor** component:
  - CodeMirror 6 instance with minimal config (Section 4).
  - Language detection from file extension.
  - `Cmd+S` / `Ctrl+S` to save (calls `code_writeFile` via IPC).
  - Auto-save on blur with debounce.
  - Updates buffer when file is externally modified (by LLM in Stage 6).
- Build **Preview** component:
  - iframe with `src="http://localhost:{port}"`.
  - Shows loading state until `codeDevServerReady`.
  - Listens for `postMessage` errors from the iframe, forwards to Console.
- Wire error forwarding script injection (the `window.addEventListener("error", ...)` snippet — either injected via Vite plugin in the scaffold template or added to the scaffold `index.html`).
- Integrate all panels into the project state context (`useReducer` for open tabs, active tab, dirty tracking).

**Test**: Open a project, see the file tree. Click `App.tsx`, see it in the editor with syntax highlighting. Edit text, save, see the change reflected in the preview iframe. Open multiple files in tabs, switch between them. Console tab shows Vite output.

**Key files touched**:
- `internal-ui/package.json` (add CodeMirror deps)
- `internal-ui/src/code.tsx` (wire layout)
- `internal-ui/src/code/layout/CodeLayout.tsx` (new)
- `internal-ui/src/code/layout/Sidebar.tsx` (new)
- `internal-ui/src/code/layout/EditorPane.tsx` (new)
- `internal-ui/src/code/filetree/FileTree.tsx`, `FileIcon.tsx` (new)
- `internal-ui/src/code/editor/CodeEditor.tsx`, `EditorTabs.tsx`, `Console.tsx` (new/update)
- `internal-ui/src/code/preview/Preview.tsx` (new)
- `internal-ui/src/code/state/project.ts`, `devserver.ts` (new)

**Estimated complexity**: High. This is the largest UI stage — many components, state management, CodeMirror integration. The split-pane layout and tab management are the most involved pieces.

---

### Stage 5: LLM Chat — Provider Abstraction, Streaming, Chat UI

**Goal**: Users can chat with Claude or OpenAI from the Code tab. The LLM can see the project context. Responses stream in real-time. No tool use yet — text-only conversation.

**Scope**:
- Build the **LLM provider abstraction** (`llm/provider.ts`):
  - Common interface: `sendMessage(config, messages, system, onChunk, onDone, onError)`.
  - Streaming via native `fetch()` + `ReadableStream`.
- Implement **Claude provider** (`llm/claude.ts`):
  - POST to `https://api.anthropic.com/v1/messages` with `stream: true`.
  - Parse SSE events (`content_block_delta`, `message_stop`, etc.).
  - Handle `anthropic-dangerous-direct-browser-access` header.
- Implement **OpenAI provider** (`llm/openai.ts`):
  - POST to `https://api.openai.com/v1/chat/completions` with `stream: true`.
  - Parse SSE `data:` lines.
- Build the **system prompt builder** (`llm/system.ts`):
  - Embeds constraints (from `constraints.md` content, hardcoded or fetched).
  - Includes current file tree (from project state).
  - Includes contents of all open editor tabs.
- Implement API key storage IPC methods: `code_getApiKeys`, `code_setApiKeys`, `code_getLlmConfig`, `code_setLlmConfig` in the Rust router + a simple JSON settings file.
- Build the **Chat UI**:
  - `ChatPane.tsx` — collapsible bottom panel with drag-to-resize handle.
  - `MessageList.tsx` — scrollable message history.
  - `Message.tsx` — renders user and assistant messages with markdown.
  - `ChatInput.tsx` — textarea with send button, `Enter` to send, `Shift+Enter` for newline.
  - Streaming indicator (animated dots or cursor) while the LLM is responding.
  - "Clear Chat" button.
  - Provider/model selector and API key settings (gear icon → inline config panel or modal).
- Build chat state management (`state/chat.ts`) — message history, streaming flag, active provider.
- Add a lightweight markdown renderer (e.g. `marked` or a minimal custom one) for assistant messages with code block syntax highlighting.

**Test**: Configure a Claude API key. Open a project. Ask "What does App.tsx do?" in the chat. See a streamed response that references the actual file contents. Switch to OpenAI, ask another question, verify it works. Clear chat, verify history resets.

**Key files touched**:
- `internal-ui/package.json` (add markdown renderer if needed)
- `internal-ui/src/code/chat/Chat.tsx`, `MessageList.tsx`, `Message.tsx`, `ChatInput.tsx` (new)
- `internal-ui/src/code/chat/llm/provider.ts`, `claude.ts`, `openai.ts`, `system.ts` (new)
- `internal-ui/src/code/state/chat.ts` (new)
- `internal-ui/src/code/layout/ChatPane.tsx` (new)
- `src/code/router.rs` (add settings methods)

**Estimated complexity**: High. Streaming SSE parsing for two different API formats, markdown rendering, and the chat UI itself. The provider abstraction needs to handle errors gracefully (rate limits, invalid keys, network failures).

---

### Stage 6: Tool Use — LLM File Editing, Auto-Apply, Diff View

**Goal**: The LLM can modify project files via tool calling. Changes are auto-applied and shown in a diff view.

**Scope**:
- Define tool schemas (`llm/tools.ts`) for `write_file` and `delete_file` (Section 6.3).
- Update the Claude and OpenAI providers to:
  - Include tools in API requests.
  - Parse tool-use blocks from streamed responses (`tool_use` content blocks for Claude, `tool_calls` for OpenAI).
  - Execute tool calls by dispatching to IPC (`code_writeFile`, `code_deleteFile`).
  - Collect tool results and send them back to the LLM for continuation (the tool-use loop).
- Build the **change tracking** system:
  - Before each `write_file`, snapshot the current file content (read via IPC or from editor cache).
  - After write, store `{ path, before, after }` in the chat state's `lastChangeSet`.
  - For `delete_file`, store `{ path, before, after: null }`.
- Build **ToolCallCard** component:
  - Collapsible card in the chat showing `[write_file: src/components/Table.tsx]`.
  - Expand to see the written content.
  - Summary line after all tool calls: `[Applied N file changes] [View Diff]`.
- Build **DiffViewer** component:
  - Opens as a new editor tab (type: Diff).
  - Shows unified diff of all files in `lastChangeSet`.
  - Red/green line highlighting in a read-only CodeMirror instance.
  - File headers with change type (created / modified / deleted).
- Update CodeMirror buffers when LLM writes to an open file (without re-triggering save).
- Auto-refresh file tree after tool calls complete.
- Add a lightweight diff computation utility (client-side, e.g. `diff` npm package or minimal Myers diff implementation).

**Test**: Ask the LLM "Add a component that shows the user's ETH balance." Verify it creates new file(s) and modifies `App.tsx`. See the changes applied in the editor tabs. Check the Diff tab shows correct red/green highlighting. Verify the preview updates via HMR. Ask a follow-up that modifies existing files, verify the diff only shows the latest turn's changes.

**Key files touched**:
- `internal-ui/package.json` (add diff library if needed)
- `internal-ui/src/code/chat/llm/tools.ts` (new)
- `internal-ui/src/code/chat/llm/claude.ts`, `openai.ts` (update for tool use)
- `internal-ui/src/code/chat/ToolCallCard.tsx` (new)
- `internal-ui/src/code/editor/DiffViewer.tsx` (new)
- `internal-ui/src/code/state/chat.ts` (add changeSet tracking)
- `internal-ui/src/code/state/project.ts` (buffer update on external write)

**Estimated complexity**: High. Tool-use streaming is the trickiest part — handling partial tool call JSON in SSE chunks, the continuation loop, and coordinating IPC writes with editor buffer updates. The diff rendering is moderately complex.

---

### Stage 7: Fork Flow — Tab Bar Button, Source Copy, Auto-Open

**Goal**: Users can fork a running dapp into VibeFi Code with one click from the tab bar.

**Scope**:
- Add a **fork button (⑂)** to the tab bar for `Standard` (dapp) tabs:
  - In `internal-ui/src/tabbar.tsx`, render the fork icon next to the close button on dapp tabs.
  - On click, send `code_forkDapp { webviewId }` IPC.
- Implement `code_forkDapp` in `src/code/project.rs`:
  - Resolve the dapp's source path from the webview's metadata (the `AppWebView` struct should store the original bundle/source path).
  - Copy source files to `<workspace_root>/<dapp-name>-fork/` (with numeric suffix for collisions).
  - Exclude `node_modules/`, `.vibefi/`, `dist/`.
  - Return the new project path.
- Handle the "source not available" case:
  - If the dapp was loaded from IPFS and only `dist/` exists, return an error.
  - The JS side shows a toast/notification: "Source not available for this dapp."
- On successful fork:
  - Switch to the Code tab.
  - Auto-load the forked project (start dev server, populate file tree, open `App.tsx`).
- Store source path metadata in `AppWebView`:
  - When loading a dapp via `--bundle`, store the bundle path.
  - When loading via IPFS registry, store the pre-build source cache path (the fetched uncompiled files).
  - Add a `source_dir: Option<PathBuf>` field to `AppWebView`.

**Test**: Launch a dapp via `--bundle`. Click ⑂ in the tab bar. Verify the Code tab opens with the forked project, dev server running, preview showing the dapp. Edit a file, verify changes show in the forked preview (not the original dapp tab). Fork the same dapp again, verify it creates `-fork-2`.

**Key files touched**:
- `internal-ui/src/tabbar.tsx` (add fork button)
- `src/code/project.rs` (implement fork logic)
- `src/code/router.rs` (add fork method)
- `src/webview_manager.rs` (add `source_dir` to `AppWebView`)
- `src/registry.rs` or `src/bundle.rs` (store source path when loading dapps)
- `src/events/user_event.rs` (add `ForkDapp`, `ForkComplete` events)

**Estimated complexity**: Medium. The main challenge is tracking source paths through the dapp loading pipeline and handling the IPFS case gracefully.

---

### Stage 8: Constraint Validator & Polish

**Goal**: Save-time validation, error surfacing, and UX polish across all features.

**Scope**:
- Implement `src/code/validator.rs`:
  - `code_validateProject` IPC method.
  - File type enforcement (check directory/extension rules from Section 8).
  - `package.json` dependency audit (compare against approved list from `constraints.md`).
  - Security pattern scan: regex search for `eval(`, `new Function(`, `innerHTML`, `dangerouslySetInnerHTML` in `.ts`/`.tsx` files.
  - `manifest.json` schema validation (capabilities structure).
  - Return `ValidationError[]` with severity, file, line, message, rule.
- Wire validation into the save flow: after each `code_writeFile`, Rust runs validation and pushes results via `codeConsoleOutput` with `source: "lint"`.
- Surface validation errors in the Console tab with yellow `[lint]` prefix.
- **Console clickable paths**: parse file:line patterns in console output, make them clickable → opens file in editor at that line.
- **Startup flow polish**:
  - Welcome screen when no API key is configured.
  - Project picker with last-opened project remembered.
  - Loading states for `bun install` and dev server startup.
- **General UX polish**:
  - Keyboard shortcuts: `Cmd+P` / `Ctrl+P` for quick file open (file picker overlay).
  - `Cmd+Shift+P` for command palette (future — stub the UI but leave empty for now).
  - Tab reordering via drag (nice-to-have).
  - Responsive layout — handle narrow windows gracefully (collapse sidebar, stack panels).
  - Error toasts for IPC failures, API errors, etc.
- **Cleanup on quit**: ensure dev server process is always killed, temp files cleaned up.

**Test**: Add an `eval()` call to a file, save, see a lint warning in the console. Add an unapproved package to `package.json`, save, see a lint error. Click on a file path in a Vite error, verify the editor jumps to the correct file and line. Verify the welcome flow works when no API key is set.

**Key files touched**:
- `src/code/validator.rs` (new)
- `src/code/router.rs` (add validate method, wire into write flow)
- `internal-ui/src/code/editor/Console.tsx` (clickable paths, lint styling)
- `internal-ui/src/code.tsx` (startup flow, welcome screen)
- Various components (loading states, error handling, keyboard shortcuts)

**Estimated complexity**: Medium. The validator is straightforward regex/JSON checking. The polish items are individually small but numerous.

---

### Stage Summary

| Stage | Name | Rust | JS | Depends On | Complexity |
|---|---|---|---|---|---|
| 1 | Rust Foundation | Heavy | Minimal | — | Medium |
| 2 | Project Management | Medium | Light | Stage 1 | Medium |
| 3 | Dev Server | Heavy | Light | Stage 1 | Medium-High |
| 4 | Editor UI | None | Heavy | Stages 1, 2, 3 | High |
| 5 | LLM Chat | Light | Heavy | Stage 4 | High |
| 6 | Tool Use & Diff | None | Heavy | Stage 5 | High |
| 7 | Fork Flow | Medium | Light | Stages 2, 4 | Medium |
| 8 | Validation & Polish | Medium | Medium | All above | Medium |

**Parallelization opportunities**:
- Stages 2 and 3 can be built in parallel (both depend only on Stage 1).
- Stage 7 can begin once Stages 2 and 4 are complete, in parallel with Stages 5-6.
- Stage 5 can begin as soon as the layout from Stage 4 has the chat pane placeholder wired.

```
Stage 1 ──┬── Stage 2 ──┐
           │             ├── Stage 4 ──── Stage 5 ──── Stage 6
           └── Stage 3 ──┘       │
                                 └── Stage 7

All ──────────────────────────────────────── Stage 8
```

---

## 18. Future Enhancements (Not in v1)

- **Ollama / local LLM support**: Add `http://localhost:11434` to CSP, add provider in `llm/`.
- **Git integration**: Auto-commit on save, undo via git revert, branch per chat session.
- **Publishing to IPFS**: Build, validate, pin to IPFS, register on-chain — all from VibeFi Code.
- **Collaborative editing**: Multiple users editing the same project (likely requires a separate server).
- **LSP integration**: TypeScript language server for autocomplete, type checking, go-to-definition.
- **Multi-project**: Multiple projects open simultaneously, each with their own dev server.
- **Inline diff markers**: Show LLM changes as inline editor decorations instead of a separate diff tab.
- **Chat-to-code navigation**: Click on code references in chat messages to jump to the file/line.
- **Project templates**: Community-contributed starter templates beyond the minimal scaffold.
- **AI model routing**: Use cheaper/faster models for simple edits, more capable models for complex features.

---
