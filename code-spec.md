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
