import React, { useEffect, useMemo, useRef, useState } from "react";
import { ToolCallCard } from "./chat/ToolCallCard";
import { DiffViewer } from "./editor/DiffViewer";
import { IpcClient } from "../ipc/client";
import { PROVIDER_IDS } from "../ipc/contracts";
import type { ProviderEventPayload } from "../ipc/contracts";
import { handleHostDispatch } from "../ipc/host-dispatch";
import { ChatMessageContent } from "./ChatMessageContent";
import { CodeEditor } from "./CodeEditor";
import { CODE_PROVIDER_EVENT } from "./constants";
import { styles } from "./styles";
import type {
  FileEntry,
  SidebarPanelId,
  WorkspaceMode,
} from "./types";
import {
  asErrorMessage,
  defaultModelForProvider,
  fileNameFromPath,
  flattenFilePaths,
  formatLastModified,
  isFileTab,
  isFileTabDirty,
  isRecord,
  modelOptionsForProvider,
  parseConsolePathMatch,
  parseDevServerStatus,
  parsePort,
} from "./utils";
import { useConsole } from "./hooks/useConsole";
import { useSettings } from "./hooks/useSettings";
import { useDevServer } from "./hooks/useDevServer";
import { useProject } from "./hooks/useProject";
import { useEditor } from "./hooks/useEditor";
import { useChat } from "./hooks/useChat";

declare global {
  interface Window {
    __VibefiHostDispatch?: (message: unknown) => void;
  }
}

const client = new IpcClient();

window.__VibefiHostDispatch = (message: unknown) => {
  handleHostDispatch(message, {
    onRpcResponse: (payload) => {
      client.resolve(payload.id, payload.result ?? null, payload.error ?? null);
    },
    onProviderEvent: (payload) => {
      window.dispatchEvent(
        new CustomEvent<ProviderEventPayload>(CODE_PROVIDER_EVENT, { detail: payload })
      );
    },
  });
};

export default function App() {
  // ── UI state (stays in App) ─────────────────────────────────────────────
  const [workspaceMode, setWorkspaceMode] = useState<WorkspaceMode>("llm-code-preview");
  const [activeSidebarPanel, setActiveSidebarPanel] = useState<SidebarPanelId>("projects");
  const [quickOpenVisible, setQuickOpenVisible] = useState(false);
  const [quickOpenQuery, setQuickOpenQuery] = useState("");
  const [quickOpenIndex, setQuickOpenIndex] = useState(0);
  const [commandPaletteVisible, setCommandPaletteVisible] = useState(false);
  const [openingFile, setOpeningFile] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [previewUrl, setPreviewUrl] = useState<string | null>(null);
  const [awaitingPreviewReady, setAwaitingPreviewReady] = useState(false);
  const [previewFrameKey, setPreviewFrameKey] = useState(0);

  const quickOpenInputRef = useRef<HTMLInputElement | null>(null);

  // ── Domain hooks ────────────────────────────────────────────────────────
  const console_ = useConsole();
  const settings = useSettings(client);
  const project = useProject(client);
  const editor = useEditor(client, project.activeProjectPath, console_);
  const devServer = useDevServer(client, console_);
  const chat = useChat(client, settings, project, editor, console_);

  // ── Quick-open filtered list ────────────────────────────────────────────
  const quickOpenFiles = useMemo(() => {
    const allFiles = flattenFilePaths(project.fileTree);
    const query = quickOpenQuery.trim().toLowerCase();
    const filtered = query
      ? allFiles.filter((path) => path.toLowerCase().includes(query))
      : allFiles;
    return filtered.slice(0, 60);
  }, [project.fileTree, quickOpenQuery]);
  const providerModelOptions = useMemo(
    () => modelOptionsForProvider(settings.provider),
    [settings.provider]
  );
  const customModelValue = useMemo(() => {
    const trimmed = settings.model.trim();
    if (!trimmed) return null;
    return providerModelOptions.includes(trimmed) ? null : trimmed;
  }, [providerModelOptions, settings.model]);
  const selectedModelValue = useMemo(() => {
    const trimmed = settings.model.trim();
    if (trimmed) {
      return trimmed;
    }
    return defaultModelForProvider(settings.provider);
  }, [settings.model, settings.provider]);

  // ── Effects ─────────────────────────────────────────────────────────────
  useEffect(() => {
    void Promise.all([
      project.loadProjects().then((res) => { if (res.error) setError(res.error); }),
      devServer.loadStatus({ silent: true }),
      settings.load({ silent: true }),
    ]);
  }, []);

  useEffect(() => {
    if (workspaceMode !== "llm-preview" && activeSidebarPanel === "console") {
      setActiveSidebarPanel("projects");
    }
  }, [workspaceMode, activeSidebarPanel]);

  useEffect(() => {
    if (!quickOpenVisible) return;
    setQuickOpenIndex(0);
    window.setTimeout(() => {
      quickOpenInputRef.current?.focus();
      quickOpenInputRef.current?.select();
    }, 0);
  }, [quickOpenVisible]);

  useEffect(() => {
    if (!quickOpenVisible) return;
    if (quickOpenFiles.length === 0 && quickOpenIndex !== 0) {
      setQuickOpenIndex(0);
      return;
    }
    if (quickOpenIndex >= quickOpenFiles.length && quickOpenFiles.length > 0) {
      setQuickOpenIndex(quickOpenFiles.length - 1);
    }
  }, [quickOpenFiles, quickOpenIndex, quickOpenVisible]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (commandPaletteVisible) {
        if (event.key === "Escape") { event.preventDefault(); closeCommandPalette(); }
        return;
      }
      if (quickOpenVisible) {
        if (event.key === "Escape") { event.preventDefault(); closeQuickOpen(); return; }
        if (event.key === "ArrowDown") {
          event.preventDefault();
          setQuickOpenIndex((c) => quickOpenFiles.length === 0 ? 0 : Math.min(c + 1, quickOpenFiles.length - 1));
          return;
        }
        if (event.key === "ArrowUp") {
          event.preventDefault();
          setQuickOpenIndex((c) => Math.max(0, c - 1));
          return;
        }
        if (event.key === "Enter") {
          const target = quickOpenFiles[quickOpenIndex];
          if (target) { event.preventDefault(); void selectQuickOpenPath(target); return; }
        }
      }
      if (!(event.metaKey || event.ctrlKey) || event.altKey) return;
      const key = event.key.toLowerCase();
      if (event.shiftKey && key === "p") { event.preventDefault(); openCommandPalette(); return; }
      if (!event.shiftKey && key === "s") { event.preventDefault(); void handleSaveActiveTab(); return; }
      if (!event.shiftKey && key === "p") { event.preventDefault(); openQuickOpen(); }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [commandPaletteVisible, quickOpenFiles, quickOpenIndex, quickOpenVisible]);

  useEffect(() => {
    const onPreviewMessage = (event: MessageEvent) => {
      if (typeof event.origin !== "string" || !event.origin.startsWith("http://localhost:")) return;
      if (!isRecord(event.data)) return;

      if (event.data.type === "vibefi-code-console") {
        const level =
          typeof event.data.level === "string" && event.data.level.trim()
            ? event.data.level.trim().toLowerCase()
            : "log";
        const message =
          typeof event.data.message === "string" && event.data.message.trim()
            ? event.data.message.trim()
            : "(empty)";
        console_.append([`[preview:${level}] ${message}`]);
        return;
      }

      if (event.data.type !== "vibefi-code-error") return;
      const message =
        typeof event.data.message === "string" && event.data.message.trim()
          ? event.data.message.trim()
          : "Unknown runtime error";
      console_.append([`[runtime] ${message}`]);
      if (typeof event.data.stack === "string" && event.data.stack.trim()) {
        console_.append(event.data.stack.split("\n").map((line: string) => `[runtime] ${line}`));
      }
    };
    window.addEventListener("message", onPreviewMessage);
    return () => window.removeEventListener("message", onPreviewMessage);
  }, []);

  useEffect(() => {
    const onCodeProviderEvent = (event: Event) => {
      const customEvent = event as CustomEvent<ProviderEventPayload>;
      const payload = customEvent.detail;
      if (!payload || typeof payload !== "object") return;

      if (payload.event === "codeConsoleOutput") {
        const value = payload.value;
        if (!isRecord(value)) return;
        const sourceValue =
          typeof value.source === "string" && value.source.trim()
            ? value.source
            : typeof value.stream === "string" && value.stream.trim()
              ? value.stream
              : "log";
        const lineValue = value.line;
        const rawLine =
          typeof lineValue === "string"
            ? lineValue
            : lineValue === undefined || lineValue === null
              ? ""
              : String(lineValue);
        const lines = rawLine.replace(/\r\n/g, "\n").split("\n").map((line) => `[${sourceValue}] ${line}`);
        console_.append(lines);
        return;
      }

      if (payload.event === "codeDevServerReady") {
        const value = payload.value;
        const port = isRecord(value) ? parsePort(value.port) : null;
        const url =
          isRecord(value) && typeof value.url === "string" && value.url.trim()
            ? value.url.trim()
            : port !== null
              ? `http://localhost:${port}/`
              : null;
        devServer.setStatus({ running: true, port });
        setAwaitingPreviewReady(false);
        setPreviewUrl(url);
        if (url) setPreviewFrameKey((value) => value + 1);
        console_.append([
          port !== null ? `[system] dev server ready on localhost:${port}` : "[system] dev server ready",
        ]);
        return;
      }

      if (payload.event === "codeDevServerExit") {
        const value = payload.value;
        const exitCode =
          isRecord(value) && typeof value.code === "number" && Number.isFinite(value.code)
            ? Math.trunc(value.code)
            : null;
        devServer.setStatus({ running: false, port: null });
        setAwaitingPreviewReady(false);
        setPreviewUrl(null);
        console_.append([
          exitCode === null
            ? "[system] dev server exited"
            : `[system] dev server exited with code ${exitCode}`,
        ]);
        return;
      }

      if (payload.event === "codeFileChanged") {
        void editor.handleCodeFileChanged(payload.value);
        return;
      }

      if (payload.event === "codeForkComplete") {
        const value = payload.value;
        if (!isRecord(value)) return;
        const projectPath = typeof value.projectPath === "string" ? value.projectPath.trim() : "";
        if (!projectPath) return;
        setError(null);
        setStatus(`Fork created at ${projectPath}. Opening in Code...`);
        void handleOpenProject(projectPath);
      }
    };
    window.addEventListener(CODE_PROVIDER_EVENT, onCodeProviderEvent);
    return () => window.removeEventListener(CODE_PROVIDER_EVENT, onCodeProviderEvent);
  }, []);

  useEffect(() => {
    if (!devServer.status.running || devServer.status.port === null) {
      if (devServer.action !== "start") {
        setPreviewUrl(null);
      }
      return;
    }
    if (awaitingPreviewReady || previewUrl) return;
    setPreviewUrl(`http://localhost:${devServer.status.port}/`);
    setPreviewFrameKey((value) => value + 1);
  }, [
    awaitingPreviewReady,
    devServer.action,
    devServer.status.port,
    devServer.status.running,
    previewUrl,
  ]);

  useEffect(() => {
    if (!awaitingPreviewReady || !devServer.status.running || devServer.status.port === null) return;
    const fallbackTimer = window.setTimeout(() => {
      setAwaitingPreviewReady(false);
      setPreviewUrl(`http://localhost:${devServer.status.port}/`);
      setPreviewFrameKey((value) => value + 1);
    }, 3000);
    return () => window.clearTimeout(fallbackTimer);
  }, [awaitingPreviewReady, devServer.status.port, devServer.status.running]);

  async function handleOpenProject(pathOverride?: string) {
    project.setPendingAction("open");
    setError(null);
    setStatus(null);
    try {
      const opened = await project.doOpenProject(pathOverride);
      editor.applyOpenedProject();
      project.setOpenFilePathInput("src/App.tsx");
      setStatus(`Opened ${opened.projectPath}`);
      setAwaitingPreviewReady(true);
      setPreviewUrl(null);
      const dvResult = await devServer.start(opened.projectPath, { auto: true });
      if (dvResult.error) {
        setAwaitingPreviewReady(false);
        setError(dvResult.error);
      }
      const lResult = await project.loadProjects();
      if (lResult.error) console_.append([`[system] ${lResult.error}`]);
    } catch (err) {
      setAwaitingPreviewReady(false);
      setError(`Failed to open project: ${asErrorMessage(err)}`);
    } finally {
      project.setPendingAction(null);
    }
  }

  async function handleCreateProject() {
    const name = project.newProjectName.trim();
    if (!name) { setError("Project name is required."); return; }
    project.setPendingAction("create");
    setError(null);
    setStatus(null);
    try {
      const opened = await project.doCreateProject(name);
      editor.applyOpenedProject();
      project.setOpenFilePathInput("src/App.tsx");
      setAwaitingPreviewReady(true);
      setPreviewUrl(null);
      const dvResult = await devServer.start(opened.projectPath, { auto: true });
      if (dvResult.error) {
        setAwaitingPreviewReady(false);
        setError(dvResult.error);
      }
      project.setNewProjectName("");
      setStatus(`Created and opened ${opened.projectPath}`);
      const lResult = await project.loadProjects();
      if (lResult.error) console_.append([`[system] ${lResult.error}`]);
    } catch (err) {
      setAwaitingPreviewReady(false);
      setError(`Failed to create project: ${asErrorMessage(err)}`);
    } finally {
      project.setPendingAction(null);
    }
  }

  async function handleStartDevServer() {
    if (!project.activeProjectPath) return;
    setAwaitingPreviewReady(true);
    setPreviewUrl(null);
    const result = await devServer.start(project.activeProjectPath);
    if (result.error) {
      setAwaitingPreviewReady(false);
      setError(result.error);
    }
    if (result.status) setStatus(result.status);
  }

  async function handleStopDevServer() {
    const result = await devServer.stop();
    setAwaitingPreviewReady(false);
    setPreviewUrl(null);
    if (result.error) setError(result.error);
    if (result.status) setStatus(result.status);
  }

  async function handleSaveActiveTab() {
    const result = await editor.saveActiveTab({ announce: true });
    if (result.error) setError(result.error);
    if (result.status) setStatus(result.status);
  }

  async function openFileFromInput() {
    setOpeningFile(true);
    setError(null);
    setStatus(null);
    try {
      const opened = await editor.openFileTab(project.openFilePathInput, {
        showStatus: true,
        silentError: false,
        activate: true,
      });
      if (opened) setStatus(`Opened file ${project.openFilePathInput}`);
    } catch (err) {
      setError(`Failed to read ${project.openFilePathInput}: ${asErrorMessage(err)}`);
    } finally {
      setOpeningFile(false);
    }
  }

  async function handleCreateFile() {
    if (!project.activeProjectPath) { setError("Open a project before creating files."); return; }
    const suggestedPath = editor.activeFileTab?.path || project.openFilePathInput || "src/NewFile.tsx";
    const response = window.prompt("New file path (relative to project root)", suggestedPath);
    const filePath = response?.trim() ?? "";
    if (!filePath) return;
    setError(null); setStatus(null);
    try {
      await client.request(PROVIDER_IDS.code, "code_writeFile", [
        { projectPath: project.activeProjectPath, filePath, content: "" },
      ]);
      await project.refreshFileTree(project.activeProjectPath, { silent: true });
      await editor.openFileTab(filePath, { activate: true, silentError: false });
      setStatus(`Created ${filePath}`);
    } catch (err) {
      setError(`Failed to create file ${filePath}: ${asErrorMessage(err)}`);
    }
  }

  async function handleCreateFolder() {
    if (!project.activeProjectPath) { setError("Open a project before creating folders."); return; }
    const response = window.prompt("New folder path (relative to project root)", "src/components");
    const dirPath = response?.trim() ?? "";
    if (!dirPath) return;
    setError(null); setStatus(null);
    try {
      await client.request(PROVIDER_IDS.code, "code_createDir", [
        { projectPath: project.activeProjectPath, dirPath },
      ]);
      await project.refreshFileTree(project.activeProjectPath, { silent: true });
      setStatus(`Created folder ${dirPath}`);
    } catch (err) {
      setError(`Failed to create folder ${dirPath}: ${asErrorMessage(err)}`);
    }
  }

  async function handleDeleteFile() {
    if (!project.activeProjectPath) { setError("Open a project before deleting files."); return; }
    const suggestedPath = editor.activeFileTab?.path || project.openFilePathInput || "";
    const response = window.prompt("File path to delete (relative to project root)", suggestedPath);
    const filePath = response?.trim() ?? "";
    if (!filePath) return;
    if (!window.confirm(`Delete file ${filePath}?`)) return;
    setError(null); setStatus(null);
    try {
      await client.request(PROVIDER_IDS.code, "code_deleteFile", [
        { projectPath: project.activeProjectPath, filePath },
      ]);
      await project.refreshFileTree(project.activeProjectPath, { silent: true });
      setStatus(`Deleted ${filePath}`);
    } catch (err) {
      setError(`Failed to delete file ${filePath}: ${asErrorMessage(err)}`);
    }
  }

  async function handleSaveSettings() {
    const result = await settings.save();
    if (result.error) setError(result.error);
    else setStatus("Saved Code LLM settings");
  }

  // ── Quick-open / command palette ────────────────────────────────────────
  function openQuickOpen() {
    if (!project.activeProjectPath) return;
    setCommandPaletteVisible(false);
    setQuickOpenQuery("");
    setQuickOpenVisible(true);
  }
  function closeQuickOpen() { setQuickOpenVisible(false); }
  function openCommandPalette() { setQuickOpenVisible(false); setCommandPaletteVisible(true); }
  function closeCommandPalette() { setCommandPaletteVisible(false); }
  async function selectQuickOpenPath(path: string) {
    closeQuickOpen();
    setError(null); setStatus(null);
    try {
      await editor.openFileTab(path, { showStatus: true, silentError: false, activate: true });
      setStatus(`Opened file ${path}`);
    } catch (err) {
      setError(`Failed to read ${path}: ${asErrorMessage(err)}`);
    }
  }

  // ── Render helpers ──────────────────────────────────────────────────────
  function renderFileTree(entries: FileEntry[], depth = 0): React.ReactNode {
    if (entries.length === 0) return <div className="tree-empty">No files to display.</div>;
    return entries.map((entry) => {
      const leftPad = 6 + depth * 14;
      if (entry.isDir) {
        const expanded = project.expandedDirs.has(entry.path);
        return (
          <div key={entry.path}>
            <button
              className="tree-item"
              style={{ paddingLeft: `${leftPad}px` }}
              title={entry.path}
              onClick={() => project.toggleDir(entry.path)}
            >
              {expanded ? "[-]" : "[+]"} {entry.name}
            </button>
            {expanded ? renderFileTree(entry.children ?? [], depth + 1) : null}
          </div>
        );
      }
      const tabId = entry.path;
      const isActive = editor.activeFileTab?.path === entry.path;
      const sizeSuffix = typeof entry.size === "number" ? ` (${entry.size} bytes)` : "";
      return (
        <button
          key={entry.path}
          className={`tree-item ${isActive ? "active" : ""}`}
          style={{ paddingLeft: `${leftPad}px` }}
          title={`${entry.path}${sizeSuffix}`}
          onClick={() => {
            setError(null); setStatus(null);
            void editor.openFileTab(entry.path, { activate: true });
          }}
          disabled={!project.activeProjectPath || project.pendingAction !== null}
        >
          {entry.name}
        </button>
      );
    });
  }

  function renderConsoleOutput(emptyState: string): React.ReactNode {
    if (console_.lines.length === 0) return emptyState;
    return console_.lines.map((line, index) => {
      const match = parseConsolePathMatch(line);
      if (!match) {
        return <div className="console-line" key={`cl-${index}`}>{line}</div>;
      }
      const before = line.slice(0, match.start);
      const linked = line.slice(match.start, match.end);
      const after = line.slice(match.end);
      return (
        <div className="console-line" key={`cl-${index}`}>
          {before}
          <button
            className="console-link"
            onClick={() => {
              setError(null); setStatus(null);
              void editor.openFileAtLocation(match.path, match.line).then((r) => {
                if (r.error) setError(r.error);
                if (r.status) setStatus(r.status);
              });
            }}
            title={`Open ${match.path}:${match.line}`}
          >
            {linked}
          </button>
          {after}
        </div>
      );
    });
  }

  function renderSidebarPanel(): React.ReactNode {
    if (activeSidebarPanel === "projects") {
      return (
        <>
          <div className="section-head">
            <h3>Projects</h3>
            <button
              className="secondary"
              onClick={() => project.loadProjects().then((r) => { if (r.error) setError(r.error); })}
              disabled={project.loadingProjects || project.pendingAction !== null || devServer.action !== null}
            >
              {project.loadingProjects ? "Loading..." : "Refresh"}
            </button>
          </div>
          <div className="sidebar-scroll">
            <div className="panel-block">
              {project.loadingProjects ? (
                <div className="tree-empty">Loading projects...</div>
              ) : project.projects.length === 0 ? (
                <div className="tree-empty">No projects found.</div>
              ) : (
                <div className="project-list">
                  {project.projects.map((p) => (
                    <div className="project-item" key={p.path}>
                      <div>
                        <div className="project-name">{p.name}</div>
                        <div className="project-path">{p.path}</div>
                        <div className="project-meta">Last modified: {formatLastModified(p.lastModified)}</div>
                      </div>
                      <button
                        className="secondary"
                        onClick={() => void handleOpenProject(p.path)}
                        disabled={project.pendingAction !== null || devServer.action !== null}
                      >
                        Open
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </div>
            <div className="panel-block">
              <h3>New Project</h3>
              <div className="field">
                <label>Name</label>
                <input
                  value={project.newProjectName}
                  placeholder="my-vibefi-dapp"
                  onChange={(e) => project.setNewProjectName(e.target.value)}
                  onKeyDown={(e) => { if (e.key === "Enter") void handleCreateProject(); }}
                  disabled={project.pendingAction !== null || devServer.action !== null}
                />
              </div>
              <div className="actions">
                <button
                  className="primary"
                  onClick={() => void handleCreateProject()}
                  disabled={project.pendingAction !== null || devServer.action !== null}
                >
                  {project.pendingAction === "create" ? "Creating..." : "Create Project"}
                </button>
              </div>
            </div>
            <div className="panel-block">
              <h3>Open Project</h3>
              <div className="field">
                <label>Path</label>
                <input
                  value={project.projectPathInput}
                  placeholder="/path/to/project"
                  onChange={(e) => project.setProjectPathInput(e.target.value)}
                  onKeyDown={(e) => { if (e.key === "Enter") void handleOpenProject(); }}
                  disabled={project.pendingAction !== null || devServer.action !== null}
                />
              </div>
              <div className="actions">
                <button
                  className="primary"
                  onClick={() => void handleOpenProject()}
                  disabled={project.pendingAction !== null || devServer.action !== null}
                >
                  {project.pendingAction === "open" ? "Opening..." : "Open Project"}
                </button>
              </div>
            </div>
          </div>
        </>
      );
    }

    if (activeSidebarPanel === "files") {
      return (
        <>
          <div className="section-head">
            <h3>Files</h3>
            <button
              className="secondary"
              onClick={() => project.refreshFileTree(undefined, { silent: false }).then((r) => { if (r.error) setError(r.error); })}
              disabled={!project.activeProjectPath || project.pendingAction !== null}
            >
              Refresh
            </button>
          </div>
          <div className="sidebar-scroll">
            <div className="file-open-row">
              <input
                value={project.openFilePathInput}
                placeholder="src/App.tsx"
                onChange={(e) => project.setOpenFilePathInput(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter") void openFileFromInput(); }}
                disabled={!project.activeProjectPath || project.pendingAction !== null || openingFile}
              />
              <button
                className="secondary"
                onClick={() => void openFileFromInput()}
                disabled={!project.activeProjectPath || project.pendingAction !== null || openingFile}
              >
                {openingFile ? "Opening..." : "Open"}
              </button>
            </div>
            <div className="actions" style={{ marginTop: 0, marginBottom: "10px" }}>
              <button
                className="secondary"
                onClick={() => void handleCreateFile()}
                disabled={!project.activeProjectPath || project.pendingAction !== null || openingFile}
              >
                New File
              </button>
              <button
                className="secondary"
                onClick={() => void handleCreateFolder()}
                disabled={!project.activeProjectPath || project.pendingAction !== null || openingFile}
              >
                New Folder
              </button>
              <button
                className="secondary"
                onClick={() => void handleDeleteFile()}
                disabled={!project.activeProjectPath || project.pendingAction !== null || openingFile}
              >
                Delete File
              </button>
            </div>
            <div className="tree-wrap">
              {project.activeProjectPath
                ? renderFileTree(project.fileTree)
                : <div className="tree-empty">Open a project.</div>}
            </div>
          </div>
        </>
      );
    }

    if (activeSidebarPanel === "dev-server") {
      return (
        <>
          <div className="section-head">
            <h3>Dev Server</h3>
            <button
              className="secondary"
              onClick={() => devServer.loadStatus().then((r) => { if (r.error) setError(r.error); })}
              disabled={project.pendingAction !== null || devServer.action !== null}
            >
              Refresh Status
            </button>
          </div>
          <div className="sidebar-scroll">
            <div className="dev-server-status">
              Status: {devServer.status.running ? "Running" : "Stopped"}
              {devServer.status.port !== null ? (
                <> on <code>localhost:{devServer.status.port}</code></>
              ) : null}
            </div>
            {project.activeProjectPath ? (
              <div className="project-meta">Project: <code>{project.activeProjectPath}</code></div>
            ) : (
              <div className="project-meta">Open a project to enable dev server controls.</div>
            )}
            <div className="actions">
              <button
                className="primary"
                onClick={() => void handleStartDevServer()}
                disabled={
                  !project.activeProjectPath ||
                  project.pendingAction !== null ||
                  devServer.action !== null ||
                  devServer.status.running
                }
              >
                {devServer.action === "start" ? "Starting..." : "Start Server"}
              </button>
              <button
                className="secondary"
                onClick={() => void handleStopDevServer()}
                disabled={project.pendingAction !== null || devServer.action !== null || !devServer.status.running}
              >
                {devServer.action === "stop" ? "Stopping..." : "Stop Server"}
              </button>
            </div>
          </div>
        </>
      );
    }

    // console panel
    return (
      <>
        <div className="console-panel-header">
          <h3>Console</h3>
          <button className="secondary" onClick={() => console_.clear()} disabled={console_.lines.length === 0}>
            Clear
          </button>
        </div>
        <pre className="console-pre sidebar-console">{renderConsoleOutput("Waiting for console output...")}</pre>
      </>
    );
  }

  // ── JSX ─────────────────────────────────────────────────────────────────
  return (
    <>
      <style>{styles}</style>
      <div className="page-container code-page">
        <div className="ide-shell">

          {/* Topbar */}
          <div className="ide-topbar surface-card">
            <div className="ide-topbar-main">
              <h1 className="page-title">VibeFi Code</h1>
            </div>
            <div className="ide-topbar-actions">
              <div className="mode-toggle">
                <button
                  className={workspaceMode === "llm-preview" ? "active" : ""}
                  onClick={() => setWorkspaceMode("llm-preview")}
                >
                  Preview
                </button>
                <button
                  className={workspaceMode === "llm-code-preview" ? "active" : ""}
                  onClick={() => setWorkspaceMode("llm-code-preview")}
                >
                  Code + Preview
                </button>
              </div>
            </div>
          </div>

          {/* Workspace */}
          <div className="ide-workspace">

            {/* Sidebar */}
            <aside className="ide-sidebar surface-card">
              <div className="sidebar-tabs">
                <button
                  className={`sidebar-tab ${activeSidebarPanel === "projects" ? "active" : ""}`}
                  onClick={() => setActiveSidebarPanel("projects")}
                >
                  Projects
                </button>
                <button
                  className={`sidebar-tab ${activeSidebarPanel === "files" ? "active" : ""}`}
                  onClick={() => setActiveSidebarPanel("files")}
                >
                  Files
                </button>
                <button
                  className={`sidebar-tab ${activeSidebarPanel === "dev-server" ? "active" : ""}`}
                  onClick={() => setActiveSidebarPanel("dev-server")}
                >
                  Dev Server
                </button>
                {workspaceMode === "llm-preview" ? (
                  <button
                    className={`sidebar-tab ${activeSidebarPanel === "console" ? "active" : ""}`}
                    onClick={() => setActiveSidebarPanel("console")}
                  >
                    Console
                  </button>
                ) : null}
              </div>
              <div className="sidebar-panel">{renderSidebarPanel()}</div>
            </aside>

            {/* Main area: editor + preview */}
            <div className={`ide-main mode-${workspaceMode}`}>
                {workspaceMode === "llm-code-preview" ? (
                  <div className="editor-shell">
                    <div className="editor-tabs">
                      {editor.openTabs.map((tab) => {
                        const active = tab.id === editor.activeTabId;
                        const dirty = isFileTab(tab) ? isFileTabDirty(tab) : false;
                        const closable = tab.kind !== "console" && tab.kind !== "chat";
                        const tabLabel = isFileTab(tab) ? fileNameFromPath(tab.path) : tab.title;
                        const closeTitle = isFileTab(tab) ? `Close ${tab.path}` : `Close ${tab.title}`;
                        return (
                          <div
                            key={tab.id}
                            className={`editor-tab ${active ? "active" : ""}`}
                            onClick={() => void editor.activateTab(tab.id)}
                            onKeyDown={(e) => {
                              if (e.key === "Enter" || e.key === " ") {
                                e.preventDefault();
                                void editor.activateTab(tab.id);
                              }
                            }}
                            onMouseDown={(e) => {
                              if (e.button === 1 && tab.kind !== "console" && tab.kind !== "chat") {
                                e.preventDefault();
                                void editor.closeTab(tab.id);
                              }
                            }}
                            role="button"
                            tabIndex={0}
                          >
                            <span>{tabLabel}</span>
                            {dirty ? <span className="editor-dirty">*</span> : null}
                            {closable ? (
                              <button
                                className="editor-tab-close"
                                onClick={(e) => {
                                  e.preventDefault();
                                  e.stopPropagation();
                                  void editor.closeTab(tab.id);
                                }}
                                title={closeTitle}
                              >
                                x
                              </button>
                            ) : null}
                          </div>
                        );
                      })}
                    </div>

                    {editor.activeFileTab ? (
                      <>
                        <div className="editor-toolbar">
                          <div className="editor-path" title={editor.activeFileTab.path}>
                            {editor.activeFileTab.path}
                          </div>
                          <div className="actions" style={{ marginTop: 0 }}>
                            <div className="editor-status">
                              {editor.activeFileTab.isSaving
                                ? "Saving..."
                                : isFileTabDirty(editor.activeFileTab)
                                  ? "Unsaved changes"
                                  : "Saved"}
                            </div>
                            <button
                              className="primary"
                              onClick={() => void handleSaveActiveTab()}
                              disabled={
                                !project.activeProjectPath ||
                                editor.activeFileTab.isLoading ||
                                editor.activeFileTab.isSaving ||
                                !isFileTabDirty(editor.activeFileTab)
                              }
                            >
                              Save
                            </button>
                          </div>
                        </div>
                        {editor.activeFileTab.isLoading ? (
                          <div className="editor-placeholder">Loading file...</div>
                        ) : (
                          <CodeEditor
                            filePath={editor.activeFileTab.path}
                            value={editor.activeFileTab.content}
                            onChange={editor.handleActiveEditorChange}
                            onBlur={() => editor.scheduleAutoSave(editor.activeFileTab!.id)}
                            jumpToLine={
                              editor.pendingLineJump?.tabId === editor.activeFileTab.id
                                ? editor.pendingLineJump.line
                                : undefined
                            }
                            jumpNonce={
                              editor.pendingLineJump?.tabId === editor.activeFileTab.id
                                ? editor.pendingLineJump.nonce
                                : undefined
                            }
                            onJumpHandled={() => {
                              editor.setPendingLineJump((current) =>
                                current?.tabId === editor.activeFileTab!.id ? null : current
                              );
                            }}
                            readOnly={editor.activeFileTab.isSaving}
                          />
                        )}
                      </>
                    ) : editor.activeDiffTab ? (
                      <>
                        <div className="editor-toolbar">
                          <div className="editor-path">Last LLM Diff</div>
                          <div className="editor-status">
                            {editor.lastChangeSet.length} file change{editor.lastChangeSet.length === 1 ? "" : "s"}
                          </div>
                        </div>
                        <DiffViewer diffText={editor.activeDiffTab.diffText} />
                      </>
                    ) : editor.activeChatTab ? (
                      <>
                        <div className="editor-toolbar">
                          <div className="editor-path">LLM Chat</div>
                          <div className="actions" style={{ marginTop: 0 }}>
                            <span className="chat-meta">
                              {chat.streaming
                                ? chat.streamStatus ?? "Streaming..."
                                : `${chat.messages.length} msg${chat.messages.length === 1 ? "" : "s"}`}
                            </span>
                            <button
                              className="secondary"
                              onClick={() => chat.clear()}
                              disabled={chat.messages.length === 0 && !chat.streaming}
                              style={{ fontSize: "11px" }}
                            >
                              Clear
                            </button>
                            {chat.streaming ? (
                              <button className="secondary" onClick={() => chat.abort()} style={{ fontSize: "11px" }}>
                                Stop
                              </button>
                            ) : null}
                            <button
                              className={`chat-gear-btn ${settingsOpen ? "active" : ""}`}
                              onClick={() => setSettingsOpen((v) => !v)}
                              title="LLM Settings"
                            >
                              ⚙
                            </button>
                          </div>
                        </div>

                        {settingsOpen ? (
                          <div className="chat-settings-panel">
                            <div className="chat-settings-grid">
                              <div className="field">
                                <label>Claude API Key</label>
                                <input
                                  type="password"
                                  value={settings.claudeApiKey}
                                  onChange={(e) => settings.setClaudeApiKey(e.target.value)}
                                  placeholder="sk-ant-..."
                                  disabled={settings.loading || settings.saving}
                                />
                              </div>
                              <div className="field">
                                <label>OpenAI API Key</label>
                                <input
                                  type="password"
                                  value={settings.openaiApiKey}
                                  onChange={(e) => settings.setOpenaiApiKey(e.target.value)}
                                  placeholder="sk-..."
                                  disabled={settings.loading || settings.saving}
                                />
                              </div>
                              <div className="field">
                                <label>Provider</label>
                                <select
                                  value={settings.provider}
                                  onChange={settings.handleProviderSelect}
                                  disabled={settings.loading || settings.saving}
                                >
                                  <option value="claude">claude</option>
                                  <option value="openai">openai</option>
                                </select>
                              </div>
                              <div className="field">
                                <label>Model</label>
                                <select
                                  value={selectedModelValue}
                                  onChange={(e) => settings.setModel(e.target.value)}
                                  disabled={settings.loading || settings.saving}
                                >
                                  {customModelValue ? (
                                    <option value={customModelValue}>{customModelValue} (custom)</option>
                                  ) : null}
                                  {providerModelOptions.map((modelId) => (
                                    <option key={modelId} value={modelId}>
                                      {modelId}
                                    </option>
                                  ))}
                                </select>
                              </div>
                            </div>
                            <div className="actions" style={{ marginTop: "8px" }}>
                              <button
                                className="secondary"
                                onClick={() => settings.load().catch((err) => setError(asErrorMessage(err)))}
                                disabled={settings.loading || settings.saving}
                                style={{ fontSize: "11px" }}
                              >
                                {settings.loading ? "Loading..." : "Reload"}
                              </button>
                              <button
                                className="primary"
                                onClick={() => void handleSaveSettings()}
                                disabled={settings.loading || settings.saving}
                                style={{ fontSize: "11px" }}
                              >
                                {settings.saving ? "Saving..." : "Save"}
                              </button>
                            </div>
                          </div>
                        ) : !settings.claudeApiKey && !settings.openaiApiKey ? (
                          <div className="chat-settings-panel" style={{ padding: "4px 10px" }}>
                            <span style={{ fontSize: "11px", color: "var(--ide-text-dim)" }}>
                              No API key — click ⚙ to configure.
                            </span>
                          </div>
                        ) : null}

                          <div className="chat-shell">
                            <div className="chat-history" ref={chat.chatHistoryRef}>
                            {chat.messages.length === 0 ? (
                              <div className="chat-placeholder">Send a prompt to start chat.</div>
                            ) : (
                              chat.messages.map((message) => (
                                <div className={`chat-message ${message.role}`} key={message.id}>
                                  <ChatMessageContent message={message} />
                                  {message.role === "assistant" && (message.toolCalls?.length ?? 0) > 0 ? (
                                    <div className="tool-calls">
                                      {message.toolCalls?.map((toolCall) => (
                                        <ToolCallCard key={toolCall.id} call={toolCall} />
                                      ))}
                                    </div>
                                  ) : null}
                                  {message.role === "assistant" && (message.changeCount ?? 0) > 0 ? (
                                    <div className="chat-change-summary">
                                      [Applied {message.changeCount} file change{message.changeCount === 1 ? "" : "s"}]
                                      {message.canViewDiff ? (
                                        <button className="secondary" onClick={() => editor.openLatestDiff()}>
                                          View Diff
                                        </button>
                                      ) : null}
                                    </div>
                                  ) : null}
                                </div>
                              ))
                            )}
                            </div>

                          {chat.streaming ? (
                            <div className="chat-stream-status">
                              <span className="chat-stream-dot" />
                              <span>{chat.streamStatus ?? "Working..."}</span>
                            </div>
                          ) : null}

                          {chat.error ? (
                            <div className="status err">
                              {chat.error}
                              {!chat.streaming && chat.lastPrompt ? (
                                <button
                                  className="secondary"
                                  style={{ marginLeft: "8px" }}
                                  onClick={() => void chat.send({ textOverride: chat.lastPrompt })}
                                >
                                  Retry
                                </button>
                              ) : null}
                            </div>
                          ) : null}

                          <div className="chat-input-row">
                            <textarea
                              value={chat.input}
                              placeholder="Type a message... (Enter to send, Shift+Enter for newline)"
                              onChange={(e) => chat.setInput(e.target.value)}
                              onKeyDown={(e) => {
                                if (e.key === "Enter" && !e.shiftKey) {
                                  e.preventDefault();
                                  void chat.send();
                                }
                              }}
                              disabled={chat.streaming || settings.loading || settings.saving}
                            />
                            <button
                              className="primary"
                              onClick={() => void chat.send()}
                              disabled={
                                chat.streaming ||
                                settings.loading ||
                                settings.saving ||
                                chat.input.trim().length === 0
                              }
                            >
                              {chat.streaming ? "Sending..." : "Send"}
                            </button>
                          </div>
                        </div>
                      </>
                    ) : (
                      <>
                        <div className="editor-toolbar">
                          <div className="editor-path">Console</div>
                          <button
                            className="secondary"
                            onClick={() => console_.clear()}
                            disabled={console_.lines.length === 0}
                          >
                            Clear
                          </button>
                        </div>
                        <pre className="console-pre">
                          {renderConsoleOutput("Waiting for code dev-server output...")}
                        </pre>
                      </>
                    )}
                  </div>
                ) : null}

                {/* Preview panel */}
                <div className="preview-panel">
                  <div className="preview-toolbar">
                    <span>Live Preview</span>
                    <span>
                      {awaitingPreviewReady
                        ? "Starting..."
                        : devServer.status.running && devServer.status.port !== null
                        ? `localhost:${devServer.status.port}`
                        : "Dev server stopped"}
                    </span>
                  </div>
                  {previewUrl ? (
                    <div className="preview-frame-wrap">
                      <iframe
                        key={previewFrameKey}
                        className="preview-frame"
                        src={previewUrl}
                        title="Live project preview"
                      />
                    </div>
                  ) : (
                    <div className="preview-fallback">
                      {awaitingPreviewReady || (devServer.status.running && devServer.status.port !== null)
                        ? "Starting dev server. Preview will load when ready."
                        : "Dev server is stopped. Start the server to show a live preview."}
                    </div>
                  )}
                </div>
            </div>
          </div>
        </div>

        {/* Quick Open overlay */}
        {quickOpenVisible ? (
          <div
            className="quick-open-overlay"
            onMouseDown={(e) => { if (e.target === e.currentTarget) closeQuickOpen(); }}
          >
            <div className="quick-open-modal" onMouseDown={(e) => e.stopPropagation()}>
              <input
                ref={quickOpenInputRef}
                value={quickOpenQuery}
                placeholder="Quick Open (Ctrl/Cmd+P)"
                onChange={(e) => { setQuickOpenQuery(e.target.value); setQuickOpenIndex(0); }}
              />
              <div className="quick-open-results">
                {quickOpenFiles.length === 0 ? (
                  <div className="quick-open-empty">No files match your query.</div>
                ) : (
                  quickOpenFiles.map((filePath, index) => (
                    <button
                      className={`quick-open-result ${index === quickOpenIndex ? "active" : ""}`}
                      key={filePath}
                      onMouseEnter={() => setQuickOpenIndex(index)}
                      onClick={() => void selectQuickOpenPath(filePath)}
                    >
                      {filePath}
                    </button>
                  ))
                )}
              </div>
            </div>
          </div>
        ) : null}

        {/* Command Palette overlay */}
        {commandPaletteVisible ? (
          <div
            className="quick-open-overlay"
            onMouseDown={(e) => { if (e.target === e.currentTarget) closeCommandPalette(); }}
          >
            <div className="quick-open-modal" onMouseDown={(e) => e.stopPropagation()}>
              <input value="" placeholder="Command Palette (Ctrl/Cmd+Shift+P)" readOnly />
              <div className="quick-open-results">
                <div className="command-palette-empty">Command palette is stubbed; no commands yet.</div>
              </div>
            </div>
          </div>
        ) : null}

        {status ? <div className="status ok">{status}</div> : null}
        {error ? <div className="status err">{error}</div> : null}
      </div>
    </>
  );
}
