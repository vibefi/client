import React, { useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { EditorState, Compartment, type Extension } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, drawSelection } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { bracketMatching } from "@codemirror/language";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { html } from "@codemirror/lang-html";
import { css } from "@codemirror/lang-css";
import { searchKeymap } from "@codemirror/search";
import { oneDark } from "@codemirror/theme-one-dark";
import {
  sendChatStream,
  type ChatMessage as LlmChatMessage,
  type ChatProvider,
} from "./code/chat/llm/provider";
import { ToolCallCard, type ToolCallCardData } from "./code/chat/ToolCallCard";
import { DiffViewer } from "./code/editor/DiffViewer";
import { buildUnifiedDiffForChanges, type DiffChange as FileChange } from "./code/editor/diff";
import { buildSystemPrompt } from "./code/chat/llm/system";
import type {
  DeleteFileToolInput,
  ToolCall,
  ToolExecutionResult,
  WriteFileToolInput,
} from "./code/chat/llm/tools";
import { IpcClient } from "./ipc/client";
import { PROVIDER_IDS, type ProviderEventPayload } from "./ipc/contracts";
import { handleHostDispatch } from "./ipc/host-dispatch";
import {
  composeStyles,
  sharedFeedbackStyles,
  sharedFormFieldStyles,
  sharedPageStyles,
  sharedStyles,
  sharedSurfaceStyles,
} from "./styles/shared";

declare global {
  interface Window {
    __VibefiHostDispatch?: (message: unknown) => void;
  }
}

const client = new IpcClient();
const CODE_PROVIDER_EVENT = "vibefi:code-provider-event";
const MAX_CONSOLE_LINES = 600;
const CONSOLE_TAB_ID = "code-console";
const DIFF_TAB_ID = "code-diff";
const AUTO_SAVE_DEBOUNCE_MS = 1000;

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

type ProjectSummary = {
  name: string;
  path: string;
  lastModified?: string | number;
};

type FileEntry = {
  name: string;
  path: string;
  isDir: boolean;
  size?: number;
  children?: FileEntry[];
};

type OpenProjectResult = {
  projectPath: string;
  files: FileEntry[];
};

type DevServerStatus = {
  running: boolean;
  port: number | null;
};

type FileTab = {
  id: string;
  kind: "file";
  path: string;
  content: string;
  savedContent: string;
  isLoading: boolean;
  isSaving: boolean;
};

type ConsoleTab = {
  id: typeof CONSOLE_TAB_ID;
  kind: "console";
  title: string;
};

type DiffTab = {
  id: typeof DIFF_TAB_ID;
  kind: "diff";
  title: string;
  diffText: string;
};

type EditorTab = FileTab | ConsoleTab | DiffTab;
type ConsolePathMatch = {
  path: string;
  line: number;
  start: number;
  end: number;
};
type ChatUiMessage = {
  id: string;
  role: "user" | "assistant";
  content: string;
  toolCalls?: ToolCallCardData[];
  changeCount?: number;
  canViewDiff?: boolean;
};

const localStyles = `
  .page-container.code-page {
    max-width: 1200px;
  }
  .section {
    margin-bottom: 20px;
  }
  .section-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    margin-bottom: 10px;
  }
  .section-head h2,
  .section h2 {
    margin: 0;
    font-size: 16px;
  }
  .project-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .project-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    padding: 12px;
  }
  .project-name {
    font-size: 14px;
    font-weight: 600;
    color: #0f172a;
  }
  .project-path {
    margin-top: 3px;
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
    font-size: 12px;
    color: #334155;
    word-break: break-all;
  }
  .project-meta {
    margin-top: 3px;
    font-size: 11px;
    color: #64748b;
  }
  .panel-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
  }
  .panel {
    padding: 12px;
  }
  .panel h3 {
    margin: 0 0 10px;
    font-size: 14px;
  }
  .actions {
    margin-top: 10px;
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .dev-server-status {
    font-size: 13px;
    color: #334155;
  }
  .dev-server-status code {
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
  }
  .preview-frame-wrap {
    border: 1px solid #cbd5e1;
    border-radius: 10px;
    overflow: hidden;
    background: #f8fafc;
  }
  .preview-frame {
    display: block;
    width: 100%;
    height: 460px;
    border: 0;
    background: #ffffff;
  }
  .preview-fallback {
    min-height: 130px;
    border: 1px dashed #cbd5e1;
    border-radius: 10px;
    background: #f8fafc;
    color: #64748b;
    font-size: 13px;
    padding: 14px;
    display: flex;
    align-items: center;
    justify-content: center;
    text-align: center;
  }
  .workspace-grid {
    display: grid;
    grid-template-columns: 280px minmax(0, 1fr);
    gap: 12px;
  }
  .file-open-row {
    display: flex;
    gap: 8px;
    margin-bottom: 10px;
  }
  .file-open-row input {
    flex: 1;
    min-width: 0;
  }
  .tree-wrap {
    border: 1px solid #cbd5e1;
    border-radius: 10px;
    background: #f8fafc;
    min-height: 360px;
    max-height: 620px;
    overflow: auto;
    padding: 8px;
  }
  .tree-item {
    display: block;
    width: 100%;
    text-align: left;
    border: 0;
    background: transparent;
    color: #0f172a;
    font-size: 12px;
    padding: 6px;
    border-radius: 6px;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tree-item:hover {
    background: #e2e8f0;
  }
  .tree-item.active {
    background: #cbd5e1;
    font-weight: 600;
  }
  .tree-empty {
    color: #64748b;
    font-size: 12px;
    padding: 8px;
  }
  .editor-shell {
    border: 1px solid #cbd5e1;
    border-radius: 10px;
    background: #0f172a;
    overflow: hidden;
    min-height: 520px;
    display: flex;
    flex-direction: column;
  }
  .editor-tabs {
    display: flex;
    gap: 2px;
    align-items: stretch;
    border-bottom: 1px solid #1e293b;
    background: #0b1220;
    overflow-x: auto;
    padding: 4px;
  }
  .editor-tab {
    border: 1px solid transparent;
    background: #172033;
    color: #cbd5e1;
    border-radius: 8px;
    height: 32px;
    padding: 0 10px;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    white-space: nowrap;
    font-size: 12px;
  }
  .editor-tab.active {
    border-color: #334155;
    background: #1e293b;
    color: #f8fafc;
  }
  .editor-tab-close {
    border: 0;
    background: transparent;
    color: #94a3b8;
    cursor: pointer;
    font-size: 12px;
    padding: 0;
    line-height: 1;
  }
  .editor-tab-close:hover {
    color: #f8fafc;
  }
  .editor-dirty {
    color: #facc15;
    font-size: 12px;
  }
  .editor-toolbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-bottom: 1px solid #1e293b;
    background: #111827;
  }
  .editor-path {
    color: #cbd5e1;
    font-size: 12px;
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .editor-status {
    color: #94a3b8;
    font-size: 11px;
  }
  .editor-textarea {
    flex: 1;
    width: 100%;
    resize: none;
    border: 0;
    margin: 0;
    padding: 12px;
    background: #0f172a;
    color: #e2e8f0;
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
    font-size: 12px;
    line-height: 1.5;
    min-height: 360px;
    outline: none;
  }
  .editor-codemirror {
    flex: 1;
    min-height: 360px;
    overflow: auto;
  }
  .editor-codemirror .cm-editor {
    height: 100%;
    background: #0f172a;
  }
  .editor-codemirror .cm-scroller {
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
    font-size: 12px;
    line-height: 1.5;
    min-height: 360px;
  }
  .editor-codemirror .cm-content {
    padding: 12px;
  }
  .editor-codemirror .cm-gutters {
    background: #0f172a;
    border-right: 1px solid #1e293b;
    color: #64748b;
  }
  .editor-placeholder {
    color: #94a3b8;
    padding: 16px;
    font-size: 12px;
  }
  .console-pre {
    margin: 0;
    flex: 1;
    overflow: auto;
    padding: 12px;
    font-size: 12px;
    color: #e2e8f0;
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .console-line {
    white-space: pre-wrap;
    word-break: break-word;
  }
  .console-link {
    border: 0;
    background: transparent;
    color: #7dd3fc;
    cursor: pointer;
    font: inherit;
    padding: 0;
    text-decoration: underline;
  }
  .console-link:hover {
    color: #bae6fd;
  }
  .diff-pre {
    margin: 0;
    flex: 1;
    overflow: auto;
    padding: 12px;
    font-size: 12px;
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
    white-space: pre;
    color: #e2e8f0;
    background: #0f172a;
  }
  .diff-line.file {
    color: #f8fafc;
    font-weight: 600;
  }
  .diff-line.meta {
    color: #93c5fd;
  }
  .diff-line.hunk {
    color: #c4b5fd;
  }
  .diff-line.add {
    color: #86efac;
    background: rgba(22, 163, 74, 0.12);
  }
  .diff-line.remove {
    color: #fca5a5;
    background: rgba(220, 38, 38, 0.12);
  }
  .diff-line.base {
    color: #cbd5e1;
  }
  .chat-shell {
    resize: vertical;
    overflow: auto;
    min-height: 140px;
    max-height: 420px;
    border: 1px solid #cbd5e1;
    border-radius: 10px;
    background: #f8fafc;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .chat-history {
    flex: 1;
    min-height: 60px;
    border: 1px solid #cbd5e1;
    border-radius: 8px;
    background: #ffffff;
    padding: 10px;
    font-size: 12px;
    color: #475569;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .chat-placeholder {
    color: #64748b;
    font-size: 12px;
  }
  .chat-message {
    padding: 8px 10px;
    border-radius: 8px;
    max-width: 95%;
    white-space: pre-wrap;
    word-break: break-word;
    font-size: 12px;
    line-height: 1.45;
  }
  .chat-message.user {
    align-self: flex-end;
    background: #dbeafe;
    color: #1e3a8a;
  }
  .chat-message.assistant {
    align-self: flex-start;
    background: #e2e8f0;
    color: #0f172a;
  }
  .chat-message.assistant p {
    margin: 0 0 8px;
  }
  .chat-message.assistant p:last-child {
    margin-bottom: 0;
  }
  .chat-message.assistant ul,
  .chat-message.assistant ol {
    margin: 0 0 8px 18px;
    padding: 0;
  }
  .chat-message.assistant code {
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
    font-size: 11px;
    background: #dbe4f0;
    border-radius: 4px;
    padding: 1px 4px;
  }
  .chat-message.assistant pre {
    margin: 8px 0 0;
    background: #0f172a;
    color: #e2e8f0;
    border-radius: 8px;
    padding: 8px;
    overflow: auto;
  }
  .chat-message.assistant pre code {
    background: transparent;
    color: inherit;
    padding: 0;
    font-size: 11px;
  }
  .tool-calls {
    margin-top: 8px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .tool-call-card {
    border: 1px solid #cbd5e1;
    border-radius: 8px;
    background: #f8fafc;
    padding: 6px 8px;
    font-size: 11px;
    line-height: 1.4;
  }
  .tool-call-card.ok {
    border-color: #86efac;
    background: #f0fdf4;
  }
  .tool-call-card.err {
    border-color: #fca5a5;
    background: #fef2f2;
  }
  .tool-call-toggle {
    width: 100%;
    border: 0;
    background: transparent;
    display: flex;
    align-items: center;
    gap: 8px;
    text-align: left;
    color: inherit;
    cursor: pointer;
    font: inherit;
    padding: 0;
  }
  .tool-call-output {
    margin-top: 4px;
    color: #334155;
  }
  .tool-call-content {
    margin: 6px 0 0;
    border: 1px solid #cbd5e1;
    border-radius: 6px;
    background: #ffffff;
    color: #0f172a;
    font-size: 11px;
    max-height: 140px;
    overflow: auto;
    padding: 6px;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .chat-change-summary {
    margin-top: 8px;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    border-radius: 999px;
    border: 1px solid #94a3b8;
    background: #f8fafc;
    color: #0f172a;
    font-size: 11px;
  }
  .chat-change-summary button {
    font-size: 11px;
    padding: 2px 7px;
  }
  .chat-meta-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
  }
  .chat-meta {
    font-size: 11px;
    color: #475569;
  }
  .chat-input-row {
    display: flex;
    gap: 8px;
    align-items: flex-end;
  }
  .chat-input-row textarea {
    flex: 1;
    min-height: 54px;
    resize: vertical;
  }
  .chat-settings-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px;
  }
  .chat-settings-grid .field {
    margin: 0;
  }
  .chat-settings-grid label {
    display: block;
    margin-bottom: 4px;
    font-size: 11px;
    color: #334155;
  }
  .chat-settings-grid input,
  .chat-settings-grid select {
    width: 100%;
  }
  .quick-open-overlay {
    position: fixed;
    inset: 0;
    background: rgba(2, 6, 23, 0.55);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding: 8vh 12px 12px;
    z-index: 1000;
  }
  .quick-open-modal {
    width: min(760px, 100%);
    border: 1px solid #334155;
    border-radius: 12px;
    background: #0f172a;
    box-shadow: 0 18px 40px rgba(15, 23, 42, 0.45);
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .quick-open-modal input {
    width: 100%;
    border: 1px solid #334155;
    border-radius: 8px;
    background: #111827;
    color: #e2e8f0;
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
  }
  .quick-open-results {
    border: 1px solid #334155;
    border-radius: 8px;
    background: #111827;
    max-height: 340px;
    overflow: auto;
  }
  .quick-open-result {
    width: 100%;
    border: 0;
    background: transparent;
    color: #cbd5e1;
    text-align: left;
    padding: 8px 10px;
    cursor: pointer;
    font-family: ui-monospace, Menlo, Monaco, Consolas, monospace;
    font-size: 12px;
  }
  .quick-open-result:hover,
  .quick-open-result.active {
    background: #1e293b;
  }
  .quick-open-empty {
    padding: 10px;
    color: #94a3b8;
    font-size: 12px;
  }
  .command-palette-empty {
    padding: 18px 10px;
    color: #94a3b8;
    font-size: 12px;
    text-align: center;
  }
  @media (max-width: 980px) {
    .panel-grid {
      grid-template-columns: 1fr;
    }
    .workspace-grid {
      grid-template-columns: 1fr;
    }
  }
  @media (max-width: 760px) {
    .file-open-row {
      flex-direction: column;
    }
    .chat-settings-grid {
      grid-template-columns: 1fr;
    }
  }
`;

const styles = composeStyles(
  sharedStyles,
  sharedPageStyles,
  sharedFormFieldStyles,
  sharedFeedbackStyles,
  sharedSurfaceStyles,
  localStyles
);

function asErrorMessage(error: unknown): string {
  if (error && typeof error === "object" && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string") return message;
  }
  return String(error);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object";
}

function fallbackProjectName(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/").filter(Boolean);
  return parts[parts.length - 1] || path;
}

function parseProjectsResult(value: unknown): ProjectSummary[] {
  if (!isRecord(value) || !Array.isArray(value.projects)) return [];

  return value.projects.flatMap((project) => {
    if (!isRecord(project)) return [];
    if (typeof project.path !== "string" || !project.path.trim()) return [];

    const name =
      typeof project.name === "string" && project.name.trim()
        ? project.name
        : fallbackProjectName(project.path);
    const lastModified =
      typeof project.lastModified === "string" || typeof project.lastModified === "number"
        ? project.lastModified
        : undefined;

    return [{ name, path: project.path, lastModified }];
  });
}

function parseProjectPath(value: unknown, method: string): string {
  if (isRecord(value) && typeof value.projectPath === "string" && value.projectPath.trim()) {
    return value.projectPath;
  }
  throw new Error(`${method} returned an invalid projectPath`);
}

function parseFileEntries(value: unknown): FileEntry[] {
  if (!Array.isArray(value)) {
    return [];
  }

  return value.flatMap((entry) => {
    if (!isRecord(entry)) return [];
    const name = typeof entry.name === "string" ? entry.name : "";
    const path = typeof entry.path === "string" ? entry.path : "";
    const isDir = entry.isDir === true;
    if (!name || !path) return [];

    const size = typeof entry.size === "number" && Number.isFinite(entry.size) ? entry.size : undefined;
    const children = isDir ? parseFileEntries(entry.children) : undefined;

    return [{ name, path, isDir, size, children }];
  });
}

function parseOpenProjectResult(value: unknown): OpenProjectResult {
  const projectPath = parseProjectPath(value, "code_openProject");
  const files = isRecord(value) ? parseFileEntries(value.files) : [];
  return { projectPath, files };
}

function parseListFilesResult(value: unknown): FileEntry[] {
  if (!isRecord(value)) return [];
  return parseFileEntries(value.files);
}

function parseReadFileResult(value: unknown): string {
  if (isRecord(value) && typeof value.content === "string") {
    return value.content;
  }
  throw new Error("code_readFile returned invalid content");
}

function parsePort(value: unknown): number | null {
  if (typeof value === "number" && Number.isFinite(value) && value > 0) {
    return Math.trunc(value);
  }
  if (typeof value === "string" && value.trim()) {
    const parsed = Number(value);
    if (Number.isFinite(parsed) && parsed > 0) {
      return Math.trunc(parsed);
    }
  }
  return null;
}

function parseDevServerStatus(value: unknown): DevServerStatus {
  if (!isRecord(value)) {
    return { running: false, port: null };
  }
  return {
    running: value.running === true,
    port: parsePort(value.port),
  };
}

function asOptionalString(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function normalizeChatProvider(value: string | null | undefined): ChatProvider {
  const normalized = (value ?? "").trim().toLowerCase();
  if (normalized === "openai" || normalized === "chatgpt" || normalized === "gpt") {
    return "openai";
  }
  return "claude";
}

function defaultModelForProvider(provider: ChatProvider): string {
  return provider === "openai" ? "gpt-4o" : "claude-sonnet-4-5-20250929";
}

function normalizeModelForProvider(provider: ChatProvider, model: string | null | undefined): string {
  const trimmed = (model ?? "").trim();
  if (!trimmed) {
    return defaultModelForProvider(provider);
  }

  const lowered = trimmed.toLowerCase();
  if (provider === "openai") {
    if (lowered === "chatgpt" || lowered.includes("claude")) {
      return defaultModelForProvider(provider);
    }
    return trimmed;
  }

  if (lowered.includes("chatgpt") || lowered.startsWith("gpt-") || lowered.includes("openai")) {
    return defaultModelForProvider(provider);
  }
  return trimmed;
}

function formatLastModified(value: ProjectSummary["lastModified"]): string {
  if (typeof value === "number" && Number.isFinite(value)) {
    const ms = value >= 1_000_000_000_000 ? value : value * 1000;
    const date = new Date(ms);
    return Number.isNaN(date.getTime()) ? String(value) : date.toLocaleString();
  }
  if (typeof value === "string" && value.trim()) {
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
  }
  return "Unknown";
}

function fileNameFromPath(path: string): string {
  const normalized = path.replace(/\\/g, "/").replace(/\/+$/, "");
  const parts = normalized.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? normalized;
}

function tabIdForPath(filePath: string): string {
  return `file:${filePath}`;
}

function createConsoleTab(): ConsoleTab {
  return { id: CONSOLE_TAB_ID, kind: "console", title: "Console" };
}

function createDiffTab(diffText: string): DiffTab {
  return { id: DIFF_TAB_ID, kind: "diff", title: "Diff", diffText };
}

function isFileTab(tab: EditorTab): tab is FileTab {
  return tab.kind === "file";
}

function isDiffTab(tab: EditorTab): tab is DiffTab {
  return tab.kind === "diff";
}

function isFileTabDirty(tab: FileTab): boolean {
  return tab.content !== tab.savedContent;
}

function chatMessageId(prefix: string): string {
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

function isWriteFileInput(input: ToolCall["input"]): input is WriteFileToolInput {
  return "content" in input;
}

function isDeleteFileInput(input: ToolCall["input"]): input is DeleteFileToolInput {
  return !("content" in input);
}

function parseConsolePathMatch(line: string): ConsolePathMatch | null {
  const patterns = [
    /(https?:\/\/localhost:\d+\/[^\s)]+?\.(?:ts|tsx|js|jsx|css|html|json)(?:\?[^\s):]+)?):(\d+)(?::\d+)?/,
    /([A-Za-z]:\\[^\s:()]+?\.(?:ts|tsx|js|jsx|css|html|json)):(\d+)(?::\d+)?/,
    /([A-Za-z]:\\[^\s:()]+?\.(?:ts|tsx|js|jsx|css|html|json))\((\d+)(?:,\d+)?\)/,
    /(\/[^\s:()]+?\.(?:ts|tsx|js|jsx|css|html|json)):(\d+)(?::\d+)?/,
    /((?:\.{1,2}\/)?[A-Za-z0-9_./-]+\.(?:ts|tsx|js|jsx|css|html|json)):(\d+)(?::\d+)?/,
    /((?:\.{1,2}\/)?[A-Za-z0-9_./-]+\.(?:ts|tsx|js|jsx|css|html|json))\((\d+)(?:,\d+)?\)/,
  ];

  for (const pattern of patterns) {
    const match = pattern.exec(line);
    if (!match) {
      continue;
    }

    const path = match[1]?.trim();
    const lineNumber = Number(match[2]);
    if (!path || !Number.isFinite(lineNumber) || lineNumber < 1) {
      continue;
    }

    const fullMatch = match[0] ?? "";
    const start = match.index;
    const end = start + fullMatch.length;
    return { path, line: Math.trunc(lineNumber), start, end };
  }

  return null;
}

function normalizeConsolePathForProject(path: string, projectPath: string): string {
  const trimmed = path.trim().replace(/^['"(]+|[)"']+$/g, "");
  if (!trimmed) return trimmed;

  if (/^https?:\/\//i.test(trimmed)) {
    try {
      const url = new URL(trimmed);
      if (url.hostname === "localhost") {
        return decodeURIComponent(url.pathname).replace(/^\/+/, "");
      }
    } catch {
      return trimmed;
    }
  }

  const normalizedPath = trimmed.replace(/\\/g, "/").replace(/^\.\/+/, "");
  const normalizedProjectPath = projectPath.trim().replace(/\\/g, "/").replace(/\/+$/, "");

  if (normalizedProjectPath && normalizedPath.startsWith(`${normalizedProjectPath}/`)) {
    return normalizedPath.slice(normalizedProjectPath.length + 1);
  }

  if (normalizedPath.startsWith("/")) {
    const withoutLeadingSlash = normalizedPath.slice(1);
    if (
      /^(src|abis|assets)\//.test(withoutLeadingSlash) ||
      /^(index\.html|package\.json|manifest\.json|addresses\.json|tsconfig\.json|vite\.config\.(?:ts|js|mjs|cjs))$/.test(
        withoutLeadingSlash
      )
    ) {
      return withoutLeadingSlash;
    }
  }

  return normalizedPath;
}

function positionForLine(content: string, line: number): number {
  if (line <= 1) return 0;

  let currentLine = 1;
  for (let index = 0; index < content.length; index += 1) {
    if (content[index] === "\n") {
      currentLine += 1;
      if (currentLine >= line) {
        return index + 1;
      }
    }
  }
  return content.length;
}

function flattenFilePaths(entries: FileEntry[]): string[] {
  const files: string[] = [];
  for (const entry of entries) {
    if (entry.isDir) {
      files.push(...flattenFilePaths(entry.children ?? []));
      continue;
    }
    files.push(entry.path);
  }
  return files;
}

function languageExtensionFromPath(filePath: string): Extension {
  if (/\.tsx?$/i.test(filePath)) {
    return javascript({ typescript: true, jsx: true });
  }
  if (/\.jsx?$/i.test(filePath)) {
    return javascript({ jsx: true });
  }
  if (/\.json$/i.test(filePath)) {
    return json();
  }
  if (/\.html?$/i.test(filePath)) {
    return html();
  }
  if (/\.css$/i.test(filePath)) {
    return css();
  }
  return [];
}

type CodeEditorProps = {
  filePath: string;
  value: string;
  onChange: (value: string) => void;
  onBlur: () => void;
  jumpToLine?: number;
  jumpNonce?: number;
  onJumpHandled?: () => void;
  readOnly?: boolean;
};

function CodeEditor({
  filePath,
  value,
  onChange,
  onBlur,
  jumpToLine,
  jumpNonce,
  onJumpHandled,
  readOnly = false,
}: CodeEditorProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  const onChangeRef = useRef(onChange);
  const onBlurRef = useRef(onBlur);
  const readOnlyCompartmentRef = useRef(new Compartment());
  const languageCompartmentRef = useRef(new Compartment());

  useEffect(() => {
    onChangeRef.current = onChange;
  }, [onChange]);

  useEffect(() => {
    onBlurRef.current = onBlur;
  }, [onBlur]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const readOnlyCompartment = readOnlyCompartmentRef.current;
    const languageCompartment = languageCompartmentRef.current;
    const baseExtensions = [
      lineNumbers(),
      history(),
      drawSelection(),
      bracketMatching(),
      keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap, indentWithTab]),
      oneDark,
      EditorView.updateListener.of((update) => {
        if (!update.docChanged) return;
        onChangeRef.current(update.state.doc.toString());
      }),
      EditorView.domEventHandlers({
        blur: () => {
          onBlurRef.current();
        },
      }),
      readOnlyCompartment.of(EditorState.readOnly.of(readOnly)),
      languageCompartment.of(languageExtensionFromPath(filePath)),
    ];

    const state = EditorState.create({
      doc: value,
      extensions: baseExtensions,
    });

    const view = new EditorView({
      state,
      parent: container,
    });
    view.contentDOM.spellcheck = false;
    viewRef.current = view;

    return () => {
      view.destroy();
      viewRef.current = null;
    };
  }, []);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;

    const effects = [];
    effects.push(
      readOnlyCompartmentRef.current.reconfigure(EditorState.readOnly.of(readOnly)),
      languageCompartmentRef.current.reconfigure(languageExtensionFromPath(filePath))
    );

    const currentDoc = view.state.doc.toString();
    if (currentDoc !== value) {
      view.dispatch({
        changes: { from: 0, to: currentDoc.length, insert: value },
        effects,
      });
      return;
    }

    view.dispatch({ effects });
  }, [filePath, readOnly, value]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view || !jumpNonce || !jumpToLine || jumpToLine < 1) {
      return;
    }

    const content = view.state.doc.toString();
    const anchor = positionForLine(content, jumpToLine);
    view.dispatch({
      selection: { anchor },
      scrollIntoView: true,
    });
    view.focus();
    onJumpHandled?.();
  }, [jumpNonce, jumpToLine, onJumpHandled]);

  return <div className="editor-codemirror" ref={containerRef} />;
}

type ChatMessageContentProps = {
  message: ChatUiMessage;
};

function ChatMessageContent({ message }: ChatMessageContentProps) {
  const content = message.content || (message.role === "assistant" ? "..." : "");
  if (message.role !== "assistant") {
    return <>{content}</>;
  }

  return (
    <ReactMarkdown remarkPlugins={[remarkGfm]} skipHtml>
      {content}
    </ReactMarkdown>
  );
}

function App() {
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [loadingProjects, setLoadingProjects] = useState(true);
  const [pendingAction, setPendingAction] = useState<"create" | "open" | null>(null);
  const [newProjectName, setNewProjectName] = useState("");
  const [projectPathInput, setProjectPathInput] = useState("");
  const [activeProjectPath, setActiveProjectPath] = useState("");
  const [fileTree, setFileTree] = useState<FileEntry[]>([]);
  const [expandedDirs, setExpandedDirs] = useState<Set<string>>(new Set());
  const [openFilePathInput, setOpenFilePathInput] = useState("src/App.tsx");
  const [openTabs, setOpenTabs] = useState<EditorTab[]>([createConsoleTab()]);
  const [activeTabId, setActiveTabId] = useState<string>(CONSOLE_TAB_ID);
  const [pendingLineJump, setPendingLineJump] = useState<{
    tabId: string;
    line: number;
    nonce: number;
  } | null>(null);
  const [quickOpenVisible, setQuickOpenVisible] = useState(false);
  const [quickOpenQuery, setQuickOpenQuery] = useState("");
  const [quickOpenIndex, setQuickOpenIndex] = useState(0);
  const [commandPaletteVisible, setCommandPaletteVisible] = useState(false);
  const [chatCollapsed, setChatCollapsed] = useState(false);
  const [claudeApiKey, setClaudeApiKey] = useState("");
  const [openaiApiKey, setOpenaiApiKey] = useState("");
  const [llmProvider, setLlmProvider] = useState<ChatProvider>("claude");
  const [llmModel, setLlmModel] = useState(defaultModelForProvider("claude"));
  const [codeSettingsLoading, setCodeSettingsLoading] = useState(false);
  const [codeSettingsSaving, setCodeSettingsSaving] = useState(false);
  const [chatMessages, setChatMessages] = useState<ChatUiMessage[]>([]);
  const [chatInput, setChatInput] = useState("");
  const [chatStreaming, setChatStreaming] = useState(false);
  const [chatError, setChatError] = useState<string | null>(null);
  const [lastChatPrompt, setLastChatPrompt] = useState("");
  const [lastChangeSet, setLastChangeSet] = useState<FileChange[]>([]);
  const [openingFile, setOpeningFile] = useState(false);
  const [devServerStatus, setDevServerStatus] = useState<DevServerStatus>({
    running: false,
    port: null,
  });
  const [devServerAction, setDevServerAction] = useState<"start" | "stop" | null>(null);
  const [consoleLines, setConsoleLines] = useState<string[]>([]);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  function handleProviderSelect(event: React.ChangeEvent<HTMLSelectElement>) {
    const nextProvider = normalizeChatProvider(event.target.value);
    setLlmProvider(nextProvider);
    setLlmModel((currentModel) => normalizeModelForProvider(nextProvider, currentModel));
  }

  const openTabsRef = useRef<EditorTab[]>(openTabs);
  const activeTabIdRef = useRef(activeTabId);
  const activeProjectPathRef = useRef(activeProjectPath);
  const autoSaveTimerRef = useRef<number | null>(null);
  const quickOpenInputRef = useRef<HTMLInputElement | null>(null);
  const chatAbortRef = useRef<AbortController | null>(null);
  const chatMessagesRef = useRef<ChatUiMessage[]>(chatMessages);

  useEffect(() => {
    openTabsRef.current = openTabs;
  }, [openTabs]);

  useEffect(() => {
    chatMessagesRef.current = chatMessages;
  }, [chatMessages]);

  useEffect(() => {
    activeTabIdRef.current = activeTabId;
  }, [activeTabId]);

  useEffect(() => {
    activeProjectPathRef.current = activeProjectPath;
  }, [activeProjectPath]);

  useEffect(() => {
    return () => {
      chatAbortRef.current?.abort();
      if (autoSaveTimerRef.current !== null) {
        window.clearTimeout(autoSaveTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!quickOpenVisible) return;
    setQuickOpenIndex(0);
    window.setTimeout(() => {
      quickOpenInputRef.current?.focus();
      quickOpenInputRef.current?.select();
    }, 0);
  }, [quickOpenVisible]);

  const quickOpenFiles = useMemo(() => {
    const allFiles = flattenFilePaths(fileTree);
    const query = quickOpenQuery.trim().toLowerCase();
    const filtered = query
      ? allFiles.filter((path) => path.toLowerCase().includes(query))
      : allFiles;
    return filtered.slice(0, 60);
  }, [fileTree, quickOpenQuery]);

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

  const activeTab = useMemo(
    () => openTabs.find((tab) => tab.id === activeTabId) ?? null,
    [openTabs, activeTabId]
  );
  const activeFileTab = activeTab && isFileTab(activeTab) ? activeTab : null;
  const activeDiffTab = activeTab && isDiffTab(activeTab) ? activeTab : null;

  function clearAutoSaveTimer() {
    if (autoSaveTimerRef.current !== null) {
      window.clearTimeout(autoSaveTimerRef.current);
      autoSaveTimerRef.current = null;
    }
  }

  function appendConsoleLines(lines: string[]) {
    if (lines.length === 0) return;
    setConsoleLines((previous) => {
      const merged = [...previous, ...lines];
      return merged.length > MAX_CONSOLE_LINES ? merged.slice(-MAX_CONSOLE_LINES) : merged;
    });
  }

  function ensureDirExpandedForPath(filePath: string) {
    const segments = filePath
      .split("/")
      .map((segment) => segment.trim())
      .filter(Boolean);
    if (segments.length < 2) return;

    setExpandedDirs((previous) => {
      const next = new Set(previous);
      let current = "";
      for (let i = 0; i < segments.length - 1; i += 1) {
        current = current ? `${current}/${segments[i]}` : segments[i];
        next.add(current);
      }
      return next;
    });
  }

  function toggleDir(path: string) {
    setExpandedDirs((previous) => {
      const next = new Set(previous);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }

  async function loadProjects() {
    setLoadingProjects(true);
    try {
      const result = await client.request(PROVIDER_IDS.code, "code_listProjects", [{}]);
      setProjects(parseProjectsResult(result));
    } catch (error) {
      setProjects([]);
      setError(`Failed to load projects: ${asErrorMessage(error)}`);
    } finally {
      setLoadingProjects(false);
    }
  }

  async function loadCodeSettings(options: { silent?: boolean } = {}) {
    setCodeSettingsLoading(true);
    try {
      const [apiKeysResult, llmConfigResult] = await Promise.all([
        client.request(PROVIDER_IDS.code, "code_getApiKeys", [{}]),
        client.request(PROVIDER_IDS.code, "code_getLlmConfig", [{}]),
      ]);

      if (isRecord(apiKeysResult)) {
        setClaudeApiKey(asOptionalString(apiKeysResult.claude) ?? "");
        setOpenaiApiKey(asOptionalString(apiKeysResult.openai) ?? "");
      }

      if (isRecord(llmConfigResult)) {
        const provider = normalizeChatProvider(asOptionalString(llmConfigResult.provider));
        setLlmProvider(provider);
        setLlmModel(normalizeModelForProvider(provider, asOptionalString(llmConfigResult.model)));
      }
    } catch (error) {
      if (!options.silent) {
        setError(`Failed to load code settings: ${asErrorMessage(error)}`);
      }
    } finally {
      setCodeSettingsLoading(false);
    }
  }

  async function saveCodeSettings() {
    setCodeSettingsSaving(true);
    setError(null);
    try {
      await client.request(PROVIDER_IDS.code, "code_setApiKeys", [
        {
          claude: claudeApiKey,
          openai: openaiApiKey,
        },
      ]);
      await client.request(PROVIDER_IDS.code, "code_setLlmConfig", [
        {
          provider: llmProvider,
          model: normalizeModelForProvider(llmProvider, llmModel),
        },
      ]);
      setStatus("Saved Code LLM settings");
    } catch (error) {
      setError(`Failed to save code settings: ${asErrorMessage(error)}`);
    } finally {
      setCodeSettingsSaving(false);
    }
  }

  function clearChat() {
    chatAbortRef.current?.abort();
    chatAbortRef.current = null;
    setChatStreaming(false);
    setChatError(null);
    setChatMessages([]);
    setLastChangeSet([]);
    setOpenTabs((previous) => previous.filter((tab) => tab.id !== DIFF_TAB_ID));
    if (activeTabIdRef.current === DIFF_TAB_ID) {
      setActiveTabId(CONSOLE_TAB_ID);
    }
  }

  function appendChatDelta(messageId: string, chunk: string) {
    if (!chunk) return;
    setChatMessages((previous) =>
      previous.map((message) =>
        message.id === messageId ? { ...message, content: `${message.content}${chunk}` } : message
      )
    );
  }

  function buildChatContextPrompt(): string {
    const openFileTabs = openTabsRef.current.filter(isFileTab).map((tab) => ({
      path: tab.path,
      content: tab.content,
    }));

    return buildSystemPrompt({
      projectPath: activeProjectPathRef.current,
      filePaths: flattenFilePaths(fileTree),
      openFiles: openFileTabs,
    });
  }

  function mapUiChatToLlmMessages(messages: ChatUiMessage[]): LlmChatMessage[] {
    return messages.map((message) => ({
      role: message.role,
      content: message.content,
    }));
  }

  function replaceOpenFileTabContent(filePath: string, content: string) {
    const tabId = tabIdForPath(filePath);
    setOpenTabs((previous) =>
      previous.map((tab) =>
        tab.id === tabId && isFileTab(tab)
          ? {
              ...tab,
              content,
              savedContent: content,
              isLoading: false,
              isSaving: false,
            }
          : tab
      )
    );
  }

  function closeOpenFileTab(filePath: string) {
    const tabId = tabIdForPath(filePath);
    setOpenTabs((previous) => previous.filter((tab) => tab.id !== tabId));
    setPendingLineJump((current) => (current?.tabId === tabId ? null : current));
    if (activeTabIdRef.current === tabId) {
      setActiveTabId(CONSOLE_TAB_ID);
    }
  }

  async function readFileSnapshot(projectPath: string, filePath: string): Promise<string | null> {
    try {
      const result = await client.request(PROVIDER_IDS.code, "code_readFile", [{ projectPath, filePath }]);
      return parseReadFileResult(result);
    } catch {
      return null;
    }
  }

  function openOrUpdateDiffTab(diffText: string, options: { activate?: boolean } = {}) {
    const nextTab = createDiffTab(diffText);
    setOpenTabs((previous) => {
      const existingIndex = previous.findIndex((tab) => tab.id === DIFF_TAB_ID);
      if (existingIndex === -1) {
        return [...previous, nextTab];
      }
      return previous.map((tab) => (tab.id === DIFF_TAB_ID ? nextTab : tab));
    });

    if (options.activate !== false) {
      setActiveTabId(DIFF_TAB_ID);
    }
  }

  function openLatestDiff() {
    if (lastChangeSet.length === 0) return;
    openOrUpdateDiffTab(buildUnifiedDiffForChanges(lastChangeSet), { activate: true });
  }

  async function sendChatMessage(options: { textOverride?: string } = {}) {
    const text = (options.textOverride ?? chatInput).trim();
    if (!text || chatStreaming || codeSettingsLoading || codeSettingsSaving) {
      return;
    }

    const provider = llmProvider;
    const apiKey = (provider === "openai" ? openaiApiKey : claudeApiKey).trim();
    if (!apiKey) {
      setChatError(
        provider === "openai"
          ? "OpenAI API key is required to send chat messages."
          : "Claude API key is required to send chat messages."
      );
      return;
    }

    const model = normalizeModelForProvider(provider, llmModel);
    if (model !== llmModel) {
      setLlmModel(model);
    }
    const userMessage: ChatUiMessage = {
      id: chatMessageId("user"),
      role: "user",
      content: text,
    };
    const assistantMessageId = chatMessageId("assistant");
    const assistantMessage: ChatUiMessage = {
      id: assistantMessageId,
      role: "assistant",
      content: "",
    };

    const toolChanges: FileChange[] = [];
    const recordToolChange = (path: string, before: string | null, after: string | null) => {
      const existingIndex = toolChanges.findIndex((candidate) => candidate.path === path);
      if (existingIndex === -1) {
        toolChanges.push({
          path,
          before,
          after,
          kind: after === null ? "delete" : before === null ? "create" : "modify",
        });
        return;
      }

      const existing = toolChanges[existingIndex];
      const merged: FileChange = {
        ...existing,
        after,
        kind: after === null ? "delete" : existing.before === null ? "create" : "modify",
      };

      const noNetChange =
        (merged.after === null && merged.before === null) ||
        (typeof merged.after === "string" &&
          typeof merged.before === "string" &&
          merged.after === merged.before);

      if (noNetChange) {
        toolChanges.splice(existingIndex, 1);
      } else {
        toolChanges[existingIndex] = merged;
      }
    };

    const nextMessages = [...chatMessagesRef.current, userMessage];
    setChatMessages((previous) => [...previous, userMessage, assistantMessage]);
    if (!options.textOverride) {
      setChatInput("");
    }
    setChatError(null);
    setChatStreaming(true);
    setLastChatPrompt(text);

    const controller = new AbortController();
    chatAbortRef.current = controller;

    try {
      const result = await sendChatStream({
        provider,
        model,
        apiKey,
        systemPrompt: buildChatContextPrompt(),
        messages: mapUiChatToLlmMessages(nextMessages),
        signal: controller.signal,
        maxToolRounds: 8,
        onDelta: (chunk) => appendChatDelta(assistantMessageId, chunk),
        onToolCall: async (toolCall: ToolCall): Promise<ToolExecutionResult> => {
          const projectPath = activeProjectPathRef.current.trim();
          if (!projectPath) {
            const failedResult: ToolExecutionResult = {
              toolCallId: toolCall.id,
              name: toolCall.name,
              ok: false,
              output: "No active project is open.",
            };
            setChatMessages((previous) =>
              previous.map((message) =>
                message.id === assistantMessageId
                  ? {
                      ...message,
                      toolCalls: [
                        ...(message.toolCalls ?? []),
                        {
                          id: toolCall.id,
                          name: toolCall.name,
                          path: toolCall.input.path,
                          content: isWriteFileInput(toolCall.input)
                            ? toolCall.input.content
                            : undefined,
                          ok: false,
                          output: failedResult.output,
                        },
                      ],
                      changeCount: toolChanges.length,
                    }
                  : message
              )
            );
            return failedResult;
          }

          const targetPath = toolCall.input.path;

          try {
            if (toolCall.name === "write_file" && isWriteFileInput(toolCall.input)) {
              const before = await readFileSnapshot(projectPath, targetPath);
              await client.request(PROVIDER_IDS.code, "code_writeFile", [
                {
                  projectPath,
                  filePath: targetPath,
                  content: toolCall.input.content,
                },
              ]);

              ensureDirExpandedForPath(targetPath);
              replaceOpenFileTabContent(targetPath, toolCall.input.content);
              recordToolChange(targetPath, before, toolCall.input.content);

              const success: ToolExecutionResult = {
                toolCallId: toolCall.id,
                name: toolCall.name,
                ok: true,
                output: `Wrote ${targetPath}`,
              };

              setChatMessages((previous) =>
                previous.map((message) =>
                  message.id === assistantMessageId
                    ? {
                        ...message,
                        toolCalls: [
                          ...(message.toolCalls ?? []),
                          {
                            id: toolCall.id,
                            name: toolCall.name,
                            path: targetPath,
                            content: toolCall.input.content,
                            ok: true,
                            output: success.output,
                          },
                        ],
                        changeCount: toolChanges.length,
                      }
                    : message
                )
              );
              return success;
            }

            if (toolCall.name === "delete_file" && isDeleteFileInput(toolCall.input)) {
              const before = await readFileSnapshot(projectPath, targetPath);
              await client.request(PROVIDER_IDS.code, "code_deleteFile", [
                {
                  projectPath,
                  filePath: targetPath,
                },
              ]);

              closeOpenFileTab(targetPath);
              recordToolChange(targetPath, before, null);

              const success: ToolExecutionResult = {
                toolCallId: toolCall.id,
                name: toolCall.name,
                ok: true,
                output: `Deleted ${targetPath}`,
              };

              setChatMessages((previous) =>
                previous.map((message) =>
                  message.id === assistantMessageId
                    ? {
                        ...message,
                        toolCalls: [
                          ...(message.toolCalls ?? []),
                          {
                            id: toolCall.id,
                            name: toolCall.name,
                            path: targetPath,
                            ok: true,
                            output: success.output,
                          },
                        ],
                        changeCount: toolChanges.length,
                      }
                    : message
                )
              );
              return success;
            }

            throw new Error(`Unsupported tool call: ${toolCall.name}`);
          } catch (error) {
            const failedResult: ToolExecutionResult = {
              toolCallId: toolCall.id,
              name: toolCall.name,
              ok: false,
              output: asErrorMessage(error),
            };

            setChatMessages((previous) =>
              previous.map((message) =>
                message.id === assistantMessageId
                  ? {
                      ...message,
                      toolCalls: [
                        ...(message.toolCalls ?? []),
                        {
                          id: toolCall.id,
                          name: toolCall.name,
                          path: targetPath,
                          content: isWriteFileInput(toolCall.input) ? toolCall.input.content : undefined,
                          ok: false,
                          output: failedResult.output,
                        },
                      ],
                      changeCount: toolChanges.length,
                    }
                  : message
              )
            );

            return failedResult;
          }
        },
      });

      setChatMessages((previous) =>
        previous.map((message) =>
          message.id === assistantMessageId
            ? {
                ...message,
                changeCount: toolChanges.length,
                canViewDiff: toolChanges.length > 0,
              }
            : message
        )
      );

      const nextChangeSet = [...toolChanges];
      setLastChangeSet(nextChangeSet);
      if (nextChangeSet.length > 0) {
        openOrUpdateDiffTab(buildUnifiedDiffForChanges(nextChangeSet), { activate: true });
      }

      if (result.toolResults.length > 0) {
        await refreshFileTree(undefined, { silent: true });
      }
    } catch (error) {
      const message = asErrorMessage(error);
      const isAbort =
        (typeof DOMException !== "undefined" &&
          error instanceof DOMException &&
          error.name === "AbortError") ||
        message.includes("AbortError");
      if (!isAbort) {
        setChatError(message);
        appendChatDelta(
          assistantMessageId,
          message ? `\n\n[error] ${message}` : "\n\n[error] Chat request failed"
        );
      } else {
        setChatError("Chat request canceled.");
        setChatMessages((previous) => {
          const target = previous.find((entry) => entry.id === assistantMessageId);
          if (!target) return previous;
          const hasVisibleContent = Boolean(target.content.trim()) || (target.toolCalls?.length ?? 0) > 0;
          if (hasVisibleContent) return previous;
          return previous.filter((entry) => entry.id !== assistantMessageId);
        });
      }
    } finally {
      chatAbortRef.current = null;
      setChatStreaming(false);
    }
  }

  async function refreshFileTree(
    projectPathOverride?: string,
    options: { silent?: boolean } = {}
  ): Promise<void> {
    const projectPath = (projectPathOverride ?? activeProjectPathRef.current).trim();
    if (!projectPath) return;

    try {
      const result = await client.request(PROVIDER_IDS.code, "code_listFiles", [{ projectPath }]);
      setFileTree(parseListFilesResult(result));
    } catch (error) {
      if (!options.silent) {
        setError(`Failed to refresh file tree: ${asErrorMessage(error)}`);
      }
    }
  }

  function applyOpenedProject(result: OpenProjectResult) {
    const initialExpanded = new Set<string>();
    result.files.forEach((entry) => {
      if (entry.isDir) {
        initialExpanded.add(entry.path);
      }
    });

    setActiveProjectPath(result.projectPath);
    setProjectPathInput(result.projectPath);
    setFileTree(result.files);
    setExpandedDirs(initialExpanded);
    setOpenTabs([createConsoleTab()]);
    setActiveTabId(CONSOLE_TAB_ID);
    setOpenFilePathInput("src/App.tsx");
    setPendingLineJump(null);
    setLastChangeSet([]);
  }

  async function saveFileTab(
    tabId: string,
    options: { announce?: boolean; silentError?: boolean } = {}
  ): Promise<boolean> {
    const projectPath = activeProjectPathRef.current.trim();
    if (!projectPath) {
      if (!options.silentError) {
        setError("Open a project before saving files.");
      }
      return false;
    }

    const tab = openTabsRef.current.find((candidate) => candidate.id === tabId);
    if (!tab || !isFileTab(tab) || tab.isLoading || tab.isSaving) {
      return false;
    }

    if (!isFileTabDirty(tab)) {
      if (options.announce) {
        setStatus(`No changes to save for ${tab.path}`);
      }
      return true;
    }

    const contentToSave = tab.content;
    setOpenTabs((previous) =>
      previous.map((candidate) =>
        candidate.id === tabId && isFileTab(candidate)
          ? { ...candidate, isSaving: true }
          : candidate
      )
    );

    try {
      await client.request(PROVIDER_IDS.code, "code_writeFile", [
        { projectPath, filePath: tab.path, content: contentToSave },
      ]);

      setOpenTabs((previous) =>
        previous.map((candidate) =>
          candidate.id === tabId && isFileTab(candidate)
            ? { ...candidate, savedContent: contentToSave, isSaving: false }
            : candidate
        )
      );

      if (options.announce) {
        setStatus(`Saved ${tab.path}`);
      }
      await refreshFileTree(projectPath, { silent: true });
      return true;
    } catch (error) {
      setOpenTabs((previous) =>
        previous.map((candidate) =>
          candidate.id === tabId && isFileTab(candidate)
            ? { ...candidate, isSaving: false }
            : candidate
        )
      );
      if (!options.silentError) {
        setError(`Failed to save ${tab.path}: ${asErrorMessage(error)}`);
      }
      return false;
    }
  }

  async function maybeAutoSaveTab(tabId: string): Promise<void> {
    const tab = openTabsRef.current.find((candidate) => candidate.id === tabId);
    if (!tab || !isFileTab(tab) || !isFileTabDirty(tab) || tab.isSaving || tab.isLoading) {
      return;
    }
    await saveFileTab(tabId, { announce: false, silentError: true });
  }

  function scheduleAutoSave(tabId: string) {
    clearAutoSaveTimer();
    autoSaveTimerRef.current = window.setTimeout(() => {
      autoSaveTimerRef.current = null;
      void maybeAutoSaveTab(tabId);
    }, AUTO_SAVE_DEBOUNCE_MS);
  }

  async function activateTab(tabId: string): Promise<void> {
    if (tabId === activeTabIdRef.current) return;

    const previousTabId = activeTabIdRef.current;
    await maybeAutoSaveTab(previousTabId);
    clearAutoSaveTimer();
    setActiveTabId(tabId);
  }

  async function closeTab(tabId: string): Promise<void> {
    if (tabId === CONSOLE_TAB_ID) return;

    const tabsBeforeClose = openTabsRef.current;
    const index = tabsBeforeClose.findIndex((tab) => tab.id === tabId);
    if (index === -1) return;

    await maybeAutoSaveTab(tabId);
    clearAutoSaveTimer();

    const remaining = tabsBeforeClose.filter((tab) => tab.id !== tabId);
    const nextFallbackTab = remaining[index] ?? remaining[index - 1] ?? remaining[0] ?? createConsoleTab();

    setOpenTabs((previous) => previous.filter((tab) => tab.id !== tabId));
    setPendingLineJump((current) => (current?.tabId === tabId ? null : current));
    if (activeTabIdRef.current === tabId) {
      setActiveTabId(nextFallbackTab.id);
    }
  }

  async function openFileTab(
    filePath: string,
    options: {
      projectPath?: string;
      showStatus?: boolean;
      silentError?: boolean;
      activate?: boolean;
    } = {}
  ): Promise<boolean> {
    const projectPath = (options.projectPath ?? activeProjectPathRef.current).trim();
    const targetFilePath = filePath.trim();

    if (!projectPath) {
      if (!options.silentError) {
        setError("Open a project before reading a file.");
      }
      return false;
    }

    if (!targetFilePath) {
      if (!options.silentError) {
        setError("File path is required.");
      }
      return false;
    }

    const tabId = tabIdForPath(targetFilePath);
    const existing = openTabsRef.current.find((tab) => tab.id === tabId);
    if (existing && isFileTab(existing)) {
      ensureDirExpandedForPath(targetFilePath);
      setOpenFilePathInput(targetFilePath);
      if (options.activate !== false) {
        await activateTab(tabId);
      }
      if (options.showStatus) {
        setStatus(`Opened file ${targetFilePath}`);
      }
      return true;
    }

    if (options.activate !== false) {
      await maybeAutoSaveTab(activeTabIdRef.current);
    }

    ensureDirExpandedForPath(targetFilePath);
    setOpenFilePathInput(targetFilePath);

    const loadingTab: FileTab = {
      id: tabId,
      kind: "file",
      path: targetFilePath,
      content: "",
      savedContent: "",
      isLoading: true,
      isSaving: false,
    };

    setOpenTabs((previous) => {
      if (previous.some((tab) => tab.id === tabId)) {
        return previous;
      }
      return [...previous, loadingTab];
    });

    if (options.activate !== false) {
      setActiveTabId(tabId);
    }

    try {
      const result = await client.request(PROVIDER_IDS.code, "code_readFile", [
        { projectPath, filePath: targetFilePath },
      ]);
      const content = parseReadFileResult(result);

      setOpenTabs((previous) =>
        previous.map((tab) =>
          tab.id === tabId && isFileTab(tab)
            ? {
                ...tab,
                content,
                savedContent: content,
                isLoading: false,
                isSaving: false,
              }
            : tab
        )
      );

      if (options.showStatus) {
        setStatus(`Opened file ${targetFilePath}`);
      }
      return true;
    } catch (error) {
      setOpenTabs((previous) => previous.filter((tab) => tab.id !== tabId));
      if (activeTabIdRef.current === tabId) {
        setActiveTabId(CONSOLE_TAB_ID);
      }
      if (!options.silentError) {
        setError(`Failed to read ${targetFilePath}: ${asErrorMessage(error)}`);
      }
      return false;
    }
  }

  async function saveActiveTab(options: { announce?: boolean } = {}): Promise<void> {
    const tabId = activeTabIdRef.current;
    const tab = openTabsRef.current.find((candidate) => candidate.id === tabId);
    if (!tab || !isFileTab(tab)) {
      return;
    }
    clearAutoSaveTimer();
    await saveFileTab(tabId, { announce: options.announce === true, silentError: false });
  }

  async function handleCodeFileChanged(value: unknown): Promise<void> {
    if (!isRecord(value)) return;

    const changedPath = typeof value.path === "string" ? value.path.trim() : "";
    const changedKind = typeof value.kind === "string" ? value.kind.trim() : "";
    if (!changedPath || !changedKind) return;

    await refreshFileTree(undefined, { silent: true });

    const tabId = tabIdForPath(changedPath);
    const openTab = openTabsRef.current.find((candidate) => candidate.id === tabId);
    if (!openTab || !isFileTab(openTab)) return;

    if (changedKind === "delete") {
      setOpenTabs((previous) => previous.filter((candidate) => candidate.id !== tabId));
      if (activeTabIdRef.current === tabId) {
        setActiveTabId(CONSOLE_TAB_ID);
      }
      return;
    }

    if (openTab.isSaving || isFileTabDirty(openTab)) {
      return;
    }

    const projectPath = activeProjectPathRef.current.trim();
    if (!projectPath) return;

    try {
      const result = await client.request(PROVIDER_IDS.code, "code_readFile", [
        { projectPath, filePath: changedPath },
      ]);
      const content = parseReadFileResult(result);
      setOpenTabs((previous) =>
        previous.map((candidate) => {
          if (candidate.id !== tabId || !isFileTab(candidate)) {
            return candidate;
          }
          if (candidate.isSaving || isFileTabDirty(candidate)) {
            return candidate;
          }
          return {
            ...candidate,
            content,
            savedContent: content,
            isLoading: false,
          };
        })
      );
    } catch (error) {
      appendConsoleLines([`[system] failed to sync changed file ${changedPath}: ${asErrorMessage(error)}`]);
    }
  }

  function handleActiveEditorChange(nextContent: string) {
    const tabId = activeTabIdRef.current;
    setOpenTabs((previous) =>
      previous.map((tab) =>
        tab.id === tabId && isFileTab(tab) ? { ...tab, content: nextContent } : tab
      )
    );
  }

  async function loadDefaultFileForProject(projectPath: string) {
    const appFileOpened = await openFileTab("src/App.tsx", {
      projectPath,
      silentError: true,
      showStatus: false,
      activate: true,
    });
    if (appFileOpened) return;

    const indexFileOpened = await openFileTab("index.html", {
      projectPath,
      silentError: true,
      showStatus: false,
      activate: true,
    });
    if (indexFileOpened) return;

    setOpenFilePathInput("src/App.tsx");
    setStatus("No default file found. Use Open File to read another path.");
  }

  async function openFileFromInput() {
    setOpeningFile(true);
    setError(null);
    setStatus(null);
    try {
      await openFileTab(openFilePathInput, { showStatus: true, silentError: false, activate: true });
    } finally {
      setOpeningFile(false);
    }
  }

  function openQuickOpen() {
    if (!activeProjectPathRef.current.trim()) {
      return;
    }
    setCommandPaletteVisible(false);
    setQuickOpenQuery("");
    setQuickOpenVisible(true);
  }

  function closeQuickOpen() {
    setQuickOpenVisible(false);
  }

  function openCommandPalette() {
    setQuickOpenVisible(false);
    setCommandPaletteVisible(true);
  }

  function closeCommandPalette() {
    setCommandPaletteVisible(false);
  }

  async function selectQuickOpenPath(path: string) {
    closeQuickOpen();
    setError(null);
    setStatus(null);
    await openFileTab(path, { showStatus: true, silentError: false, activate: true });
  }

  async function openFileAtLocation(filePath: string, line: number): Promise<void> {
    const normalizedPath = normalizeConsolePathForProject(
      filePath,
      activeProjectPathRef.current
    );
    const opened = await openFileTab(normalizedPath, {
      showStatus: true,
      silentError: false,
      activate: true,
    });
    if (!opened) return;

    setPendingLineJump({
      tabId: tabIdForPath(normalizedPath),
      line: Math.max(1, line),
      nonce: Date.now(),
    });
  }

  async function createFile() {
    const projectPath = activeProjectPathRef.current.trim();
    if (!projectPath) {
      setError("Open a project before creating files.");
      return;
    }

    const suggestedPath = activeFileTab?.path || openFilePathInput || "src/NewFile.tsx";
    const response = window.prompt("New file path (relative to project root)", suggestedPath);
    const filePath = response?.trim() ?? "";
    if (!filePath) {
      return;
    }

    setError(null);
    setStatus(null);
    try {
      await client.request(PROVIDER_IDS.code, "code_writeFile", [
        { projectPath, filePath, content: "" },
      ]);
      await refreshFileTree(projectPath, { silent: true });
      await openFileTab(filePath, { activate: true, showStatus: false, silentError: false });
      setStatus(`Created ${filePath}`);
    } catch (error) {
      setError(`Failed to create file ${filePath}: ${asErrorMessage(error)}`);
    }
  }

  async function createFolder() {
    const projectPath = activeProjectPathRef.current.trim();
    if (!projectPath) {
      setError("Open a project before creating folders.");
      return;
    }

    const response = window.prompt("New folder path (relative to project root)", "src/components");
    const dirPath = response?.trim() ?? "";
    if (!dirPath) {
      return;
    }

    setError(null);
    setStatus(null);
    try {
      await client.request(PROVIDER_IDS.code, "code_createDir", [{ projectPath, dirPath }]);
      await refreshFileTree(projectPath, { silent: true });
      setStatus(`Created folder ${dirPath}`);
    } catch (error) {
      setError(`Failed to create folder ${dirPath}: ${asErrorMessage(error)}`);
    }
  }

  async function deleteFile() {
    const projectPath = activeProjectPathRef.current.trim();
    if (!projectPath) {
      setError("Open a project before deleting files.");
      return;
    }

    const suggestedPath = activeFileTab?.path || openFilePathInput || "";
    const response = window.prompt("File path to delete (relative to project root)", suggestedPath);
    const filePath = response?.trim() ?? "";
    if (!filePath) {
      return;
    }

    const confirmed = window.confirm(`Delete file ${filePath}?`);
    if (!confirmed) {
      return;
    }

    setError(null);
    setStatus(null);
    try {
      await client.request(PROVIDER_IDS.code, "code_deleteFile", [{ projectPath, filePath }]);
      await refreshFileTree(projectPath, { silent: true });
      setStatus(`Deleted ${filePath}`);
      if (openFilePathInput.trim() === filePath) {
        setOpenFilePathInput("src/App.tsx");
      }
    } catch (error) {
      setError(`Failed to delete file ${filePath}: ${asErrorMessage(error)}`);
    }
  }

  async function loadDevServerStatus(options: { silent?: boolean } = {}) {
    try {
      const result = await client.request(PROVIDER_IDS.code, "code_devServerStatus", [{}]);
      setDevServerStatus(parseDevServerStatus(result));
    } catch (error) {
      if (!options.silent) {
        setError(`Failed to load dev server status: ${asErrorMessage(error)}`);
      }
    }
  }

  async function startDevServer(pathOverride?: string, options: { auto?: boolean } = {}) {
    const projectPath = (pathOverride ?? activeProjectPathRef.current).trim();
    if (!projectPath) {
      setError("Open a project before starting the dev server.");
      return;
    }

    setDevServerAction("start");
    appendConsoleLines([`[system] starting dev server for ${projectPath}`]);

    try {
      const result = await client.request(PROVIDER_IDS.code, "code_startDevServer", [
        { projectPath },
      ]);
      const port = isRecord(result) ? parsePort(result.port) : null;
      setDevServerStatus({ running: true, port });
      if (port !== null) {
        appendConsoleLines([`[system] dev server is running on localhost:${port}`]);
      }
      if (!options.auto) {
        setStatus(port !== null ? `Dev server running on localhost:${port}` : "Dev server started");
      }
      await loadDevServerStatus({ silent: true });
    } catch (error) {
      const message = asErrorMessage(error);
      appendConsoleLines([`[system] failed to start dev server: ${message}`]);
      setError(
        options.auto
          ? `Project opened, but dev server failed to start: ${message}`
          : `Failed to start dev server: ${message}`
      );
    } finally {
      setDevServerAction(null);
    }
  }

  async function stopDevServer() {
    setDevServerAction("stop");
    setError(null);

    try {
      await client.request(PROVIDER_IDS.code, "code_stopDevServer", [{}]);
      setDevServerStatus({ running: false, port: null });
      appendConsoleLines(["[system] dev server stopped"]);
      setStatus("Dev server stopped");
      await loadDevServerStatus({ silent: true });
    } catch (error) {
      const message = asErrorMessage(error);
      appendConsoleLines([`[system] failed to stop dev server: ${message}`]);
      setError(`Failed to stop dev server: ${message}`);
    } finally {
      setDevServerAction(null);
    }
  }

  async function openProject(pathOverride?: string) {
    const path = (pathOverride ?? projectPathInput).trim();
    setPendingAction("open");
    setError(null);
    setStatus(null);
    try {
      const params = path ? [{ path }] : [{}];
      const result = await client.request(PROVIDER_IDS.code, "code_openProject", params);
      const opened = parseOpenProjectResult(result);
      applyOpenedProject(opened);
      await loadDefaultFileForProject(opened.projectPath);
      setStatus(`Opened ${opened.projectPath}`);
      await startDevServer(opened.projectPath, { auto: true });
      await loadProjects();
    } catch (error) {
      setError(`Failed to open project: ${asErrorMessage(error)}`);
    } finally {
      setPendingAction(null);
    }
  }

  async function createProject() {
    const name = newProjectName.trim();
    if (!name) {
      setError("Project name is required.");
      return;
    }

    setPendingAction("create");
    setError(null);
    setStatus(null);
    try {
      const created = await client.request(PROVIDER_IDS.code, "code_createProject", [{ name }]);
      const projectPath = parseProjectPath(created, "code_createProject");
      const openedResult = await client.request(PROVIDER_IDS.code, "code_openProject", [
        { path: projectPath },
      ]);
      const opened = parseOpenProjectResult(openedResult);
      applyOpenedProject(opened);
      await loadDefaultFileForProject(opened.projectPath);
      await startDevServer(opened.projectPath, { auto: true });
      setNewProjectName("");
      setStatus(`Created and opened ${opened.projectPath}`);
      await loadProjects();
    } catch (error) {
      setError(`Failed to create project: ${asErrorMessage(error)}`);
    } finally {
      setPendingAction(null);
    }
  }

  useEffect(() => {
    void Promise.all([
      loadProjects(),
      loadDevServerStatus({ silent: true }),
      loadCodeSettings({ silent: true }),
    ]);
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (commandPaletteVisible) {
        if (event.key === "Escape") {
          event.preventDefault();
          closeCommandPalette();
        }
        return;
      }

      if (quickOpenVisible) {
        if (event.key === "Escape") {
          event.preventDefault();
          closeQuickOpen();
          return;
        }
        if (event.key === "ArrowDown") {
          event.preventDefault();
          setQuickOpenIndex((current) => {
            if (quickOpenFiles.length === 0) return 0;
            return Math.min(current + 1, quickOpenFiles.length - 1);
          });
          return;
        }
        if (event.key === "ArrowUp") {
          event.preventDefault();
          setQuickOpenIndex((current) => Math.max(0, current - 1));
          return;
        }
        if (event.key === "Enter") {
          const target = quickOpenFiles[quickOpenIndex];
          if (target) {
            event.preventDefault();
            void selectQuickOpenPath(target);
            return;
          }
        }
      }

      if (!(event.metaKey || event.ctrlKey) || event.shiftKey || event.altKey) {
        if (!(event.metaKey || event.ctrlKey) || event.altKey) {
          return;
        }
      }

      const key = event.key.toLowerCase();
      if (event.shiftKey && key === "p") {
        event.preventDefault();
        openCommandPalette();
        return;
      }
      if (!event.shiftKey && key === "s") {
        event.preventDefault();
        void saveActiveTab({ announce: true });
        return;
      }
      if (key === "p") {
        event.preventDefault();
        openQuickOpen();
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [commandPaletteVisible, quickOpenFiles, quickOpenIndex, quickOpenVisible]);

  useEffect(() => {
    const onPreviewMessage = (event: MessageEvent) => {
      if (typeof event.origin !== "string" || !event.origin.startsWith("http://localhost:")) {
        return;
      }
      if (!isRecord(event.data) || event.data.type !== "vibefi-code-error") {
        return;
      }

      const message =
        typeof event.data.message === "string" && event.data.message.trim()
          ? event.data.message.trim()
          : "Unknown runtime error";
      appendConsoleLines([`[runtime] ${message}`]);

      if (typeof event.data.stack === "string" && event.data.stack.trim()) {
        appendConsoleLines(event.data.stack.split("\n").map((line) => `[runtime] ${line}`));
      }
    };

    window.addEventListener("message", onPreviewMessage);
    return () => {
      window.removeEventListener("message", onPreviewMessage);
    };
  }, []);

  useEffect(() => {
    const onCodeProviderEvent = (event: Event) => {
      const customEvent = event as CustomEvent<ProviderEventPayload>;
      const payload = customEvent.detail;
      if (!payload || typeof payload !== "object") {
        return;
      }

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
        const normalized = rawLine.replace(/\r\n/g, "\n");
        const lines = normalized.split("\n").map((line) => `[${sourceValue}] ${line}`);
        appendConsoleLines(lines);
        return;
      }

      if (payload.event === "codeDevServerReady") {
        const value = payload.value;
        const port = isRecord(value) ? parsePort(value.port) : null;
        setDevServerStatus({ running: true, port });
        appendConsoleLines([
          port !== null
            ? `[system] dev server ready on localhost:${port}`
            : "[system] dev server ready",
        ]);
        return;
      }

      if (payload.event === "codeDevServerExit") {
        const value = payload.value;
        const exitCode =
          isRecord(value) && typeof value.code === "number" && Number.isFinite(value.code)
            ? Math.trunc(value.code)
            : null;
        setDevServerStatus({ running: false, port: null });
        appendConsoleLines([
          exitCode === null
            ? "[system] dev server exited"
            : `[system] dev server exited with code ${exitCode}`,
        ]);
        return;
      }

      if (payload.event === "codeFileChanged") {
        void handleCodeFileChanged(payload.value);
        return;
      }

      if (payload.event === "codeForkComplete") {
        const value = payload.value;
        if (!isRecord(value)) return;
        const projectPath = typeof value.projectPath === "string" ? value.projectPath.trim() : "";
        if (!projectPath) return;

        setError(null);
        setStatus(`Fork created at ${projectPath}. Opening in Code...`);
        void openProject(projectPath);
      }
    };

    window.addEventListener(CODE_PROVIDER_EVENT, onCodeProviderEvent);
    return () => {
      window.removeEventListener(CODE_PROVIDER_EVENT, onCodeProviderEvent);
    };
  }, []);

  function renderFileTree(entries: FileEntry[], depth = 0): React.ReactNode {
    if (entries.length === 0) {
      return <div className="tree-empty">No files to display.</div>;
    }

    return entries.map((entry) => {
      const leftPad = 6 + depth * 14;
      if (entry.isDir) {
        const expanded = expandedDirs.has(entry.path);
        return (
          <div key={entry.path}>
            <button
              className="tree-item"
              style={{ paddingLeft: `${leftPad}px` }}
              title={entry.path}
              onClick={() => toggleDir(entry.path)}
            >
              {expanded ? "[-]" : "[+]"} {entry.name}
            </button>
            {expanded ? renderFileTree(entry.children ?? [], depth + 1) : null}
          </div>
        );
      }

      const tabId = tabIdForPath(entry.path);
      const isActive = activeFileTab?.id === tabId;
      const sizeSuffix = typeof entry.size === "number" ? ` (${entry.size} bytes)` : "";

      return (
        <button
          key={entry.path}
          className={`tree-item ${isActive ? "active" : ""}`}
          style={{ paddingLeft: `${leftPad}px` }}
          title={`${entry.path}${sizeSuffix}`}
          onClick={() => {
            setError(null);
            setStatus(null);
            void openFileTab(entry.path, { showStatus: false, activate: true });
          }}
          disabled={!activeProjectPath || pendingAction !== null}
        >
          {entry.name}
        </button>
      );
    });
  }

  return (
    <>
      <style>{styles}</style>
      <div className="page-container code-page">
        <h1 className="page-title">VibeFi Code</h1>
        <div className="subtitle">Create, open, or resume a local VibeFi project.</div>

        <div className="section">
          <div className="section-head">
            <h2>Projects</h2>
            <button
              className="secondary"
              onClick={() => {
                setError(null);
                setStatus(null);
                void loadProjects();
              }}
              disabled={loadingProjects || pendingAction !== null || devServerAction !== null}
            >
              {loadingProjects ? "Loading..." : "Refresh"}
            </button>
          </div>

          {loadingProjects ? (
            <div className="empty">Loading projects...</div>
          ) : projects.length === 0 ? (
            <div className="empty">No projects found.</div>
          ) : (
            <div className="project-list">
              {projects.map((project) => (
                <div className="project-item surface-card" key={project.path}>
                  <div>
                    <div className="project-name">{project.name}</div>
                    <div className="project-path">{project.path}</div>
                    <div className="project-meta">
                      Last modified: {formatLastModified(project.lastModified)}
                    </div>
                  </div>
                  <button
                    className="secondary"
                    onClick={() => void openProject(project.path)}
                    disabled={pendingAction !== null || devServerAction !== null}
                  >
                    Open
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>

        <div className="section panel-grid">
          <div className="panel surface-card">
            <h3>New Project</h3>
            <div className="field">
              <label>Name</label>
              <input
                value={newProjectName}
                placeholder="my-vibefi-dapp"
                onChange={(event) => setNewProjectName(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") void createProject();
                }}
                disabled={pendingAction !== null || devServerAction !== null}
              />
            </div>
            <div className="actions">
              <button
                className="primary"
                onClick={() => void createProject()}
                disabled={pendingAction !== null || devServerAction !== null}
              >
                {pendingAction === "create" ? "Creating..." : "Create Project"}
              </button>
            </div>
          </div>

          <div className="panel surface-card">
            <h3>Open Project</h3>
            <div className="field">
              <label>Path</label>
              <input
                value={projectPathInput}
                placeholder="/path/to/project"
                onChange={(event) => setProjectPathInput(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") void openProject();
                }}
                disabled={pendingAction !== null || devServerAction !== null}
              />
            </div>
            <div className="actions">
              <button
                className="primary"
                onClick={() => void openProject()}
                disabled={pendingAction !== null || devServerAction !== null}
              >
                {pendingAction === "open" ? "Opening..." : "Open Project"}
              </button>
            </div>
          </div>
        </div>

        <div className="section panel-grid">
          <div className="panel surface-card">
            <h3>Dev Server</h3>
            <div className="dev-server-status">
              Status: {devServerStatus.running ? "Running" : "Stopped"}
              {devServerStatus.port !== null ? (
                <>
                  {" "}
                  on <code>localhost:{devServerStatus.port}</code>
                </>
              ) : null}
            </div>
            {activeProjectPath ? (
              <div className="project-meta">
                Project: <code>{activeProjectPath}</code>
              </div>
            ) : (
              <div className="project-meta">Open a project to enable dev server controls.</div>
            )}
            <div className="actions">
              <button
                className="primary"
                onClick={() => void startDevServer()}
                disabled={
                  !activeProjectPath ||
                  pendingAction !== null ||
                  devServerAction !== null ||
                  devServerStatus.running
                }
              >
                {devServerAction === "start" ? "Starting..." : "Start Server"}
              </button>
              <button
                className="secondary"
                onClick={() => void stopDevServer()}
                disabled={pendingAction !== null || devServerAction !== null || !devServerStatus.running}
              >
                {devServerAction === "stop" ? "Stopping..." : "Stop Server"}
              </button>
              <button
                className="secondary"
                onClick={() => void loadDevServerStatus()}
                disabled={pendingAction !== null || devServerAction !== null}
              >
                Refresh Status
              </button>
            </div>
          </div>

          <div className="panel surface-card">
            <h3>Preview</h3>
            {devServerStatus.running && devServerStatus.port !== null ? (
              <div className="preview-frame-wrap">
                <iframe
                  className="preview-frame"
                  src={`http://localhost:${devServerStatus.port}`}
                  title="Live project preview"
                />
              </div>
            ) : (
              <div className="preview-fallback">
                Dev server is stopped. Start the server to show a live preview.
              </div>
            )}
          </div>
        </div>

        <div className="section workspace-grid">
          <div className="panel surface-card">
            <div className="section-head">
              <h3>Files</h3>
              <button
                className="secondary"
                onClick={() => void refreshFileTree(undefined, { silent: false })}
                disabled={!activeProjectPath || pendingAction !== null}
              >
                Refresh
              </button>
            </div>

            <div className="file-open-row">
              <input
                value={openFilePathInput}
                placeholder="src/App.tsx"
                onChange={(event) => setOpenFilePathInput(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") void openFileFromInput();
                }}
                disabled={!activeProjectPath || pendingAction !== null || openingFile}
              />
              <button
                className="secondary"
                onClick={() => void openFileFromInput()}
                disabled={!activeProjectPath || pendingAction !== null || openingFile}
              >
                {openingFile ? "Opening..." : "Open"}
              </button>
            </div>

            <div className="actions" style={{ marginTop: 0, marginBottom: "10px" }}>
              <button
                className="secondary"
                onClick={() => void createFile()}
                disabled={!activeProjectPath || pendingAction !== null || openingFile}
              >
                New File
              </button>
              <button
                className="secondary"
                onClick={() => void createFolder()}
                disabled={!activeProjectPath || pendingAction !== null || openingFile}
              >
                New Folder
              </button>
              <button
                className="secondary"
                onClick={() => void deleteFile()}
                disabled={!activeProjectPath || pendingAction !== null || openingFile}
              >
                Delete File
              </button>
            </div>

            <div className="tree-wrap">
              {activeProjectPath ? renderFileTree(fileTree) : <div className="tree-empty">Open a project.</div>}
            </div>
          </div>

          <div className="panel surface-card">
            <h3>Editor</h3>
            <div className="editor-shell">
              <div className="editor-tabs">
                {openTabs.map((tab) => {
                  const active = tab.id === activeTabId;
                  const dirty = isFileTab(tab) ? isFileTabDirty(tab) : false;
                  const closable = tab.kind !== "console";
                  const tabLabel = isFileTab(tab) ? fileNameFromPath(tab.path) : tab.title;
                  const closeTitle = isFileTab(tab) ? `Close ${tab.path}` : `Close ${tab.title}`;

                  return (
                    <div
                      key={tab.id}
                      className={`editor-tab ${active ? "active" : ""}`}
                      onClick={() => void activateTab(tab.id)}
                      onKeyDown={(event) => {
                        if (event.key === "Enter" || event.key === " ") {
                          event.preventDefault();
                          void activateTab(tab.id);
                        }
                      }}
                      onMouseDown={(event) => {
                        if (event.button === 1 && tab.kind !== "console") {
                          event.preventDefault();
                          void closeTab(tab.id);
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
                          onClick={(event) => {
                            event.preventDefault();
                            event.stopPropagation();
                            void closeTab(tab.id);
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

              {activeFileTab ? (
                <>
                  <div className="editor-toolbar">
                    <div className="editor-path" title={activeFileTab.path}>
                      {activeFileTab.path}
                    </div>
                    <div className="actions" style={{ marginTop: 0 }}>
                      <div className="editor-status">
                        {activeFileTab.isSaving
                          ? "Saving..."
                          : isFileTabDirty(activeFileTab)
                            ? "Unsaved changes"
                            : "Saved"}
                      </div>
                      <button
                        className="primary"
                        onClick={() => void saveActiveTab({ announce: true })}
                        disabled={
                          !activeProjectPath ||
                          activeFileTab.isLoading ||
                          activeFileTab.isSaving ||
                          !isFileTabDirty(activeFileTab)
                        }
                      >
                        Save
                      </button>
                    </div>
                  </div>

                  {activeFileTab.isLoading ? (
                    <div className="editor-placeholder">Loading file...</div>
                  ) : (
                    <CodeEditor
                      filePath={activeFileTab.path}
                      value={activeFileTab.content}
                      onChange={handleActiveEditorChange}
                      onBlur={() => {
                        scheduleAutoSave(activeFileTab.id);
                      }}
                      jumpToLine={
                        pendingLineJump?.tabId === activeFileTab.id ? pendingLineJump.line : undefined
                      }
                      jumpNonce={
                        pendingLineJump?.tabId === activeFileTab.id ? pendingLineJump.nonce : undefined
                      }
                      onJumpHandled={() => {
                        setPendingLineJump((current) =>
                          current?.tabId === activeFileTab.id ? null : current
                        );
                      }}
                      readOnly={activeFileTab.isSaving}
                    />
                  )}
                </>
              ) : activeDiffTab ? (
                <>
                  <div className="editor-toolbar">
                    <div className="editor-path">Last LLM Diff</div>
                    <div className="editor-status">
                      {lastChangeSet.length} file change{lastChangeSet.length === 1 ? "" : "s"}
                    </div>
                  </div>
                  <DiffViewer diffText={activeDiffTab.diffText} />
                </>
              ) : (
                <>
                  <div className="editor-toolbar">
                    <div className="editor-path">Console</div>
                    <button
                      className="secondary"
                      onClick={() => setConsoleLines([])}
                      disabled={consoleLines.length === 0}
                    >
                      Clear
                    </button>
                  </div>
                  <pre className="console-pre">
                    {consoleLines.length > 0
                      ? consoleLines.map((line, index) => {
                          const match = parseConsolePathMatch(line);
                          if (!match) {
                            return (
                              <div className="console-line" key={`console-line-${index}`}>
                                {line}
                              </div>
                            );
                          }

                          const before = line.slice(0, match.start);
                          const linked = line.slice(match.start, match.end);
                          const after = line.slice(match.end);

                          return (
                            <div className="console-line" key={`console-line-${index}`}>
                              {before}
                              <button
                                className="console-link"
                                onClick={() => {
                                  setError(null);
                                  setStatus(null);
                                  void openFileAtLocation(match.path, match.line);
                                }}
                                title={`Open ${match.path}:${match.line}`}
                              >
                                {linked}
                              </button>
                              {after}
                            </div>
                          );
                        })
                      : "Waiting for code dev-server output..."}
                  </pre>
                </>
              )}
            </div>
          </div>
        </div>

        <div className="section">
          <div className="panel surface-card">
            <div className="section-head">
              <h3>LLM Chat</h3>
              <button className="secondary" onClick={() => setChatCollapsed((current) => !current)}>
                {chatCollapsed ? "Expand" : "Collapse"}
              </button>
            </div>
            {chatCollapsed ? (
              <div className="project-meta">Chat panel collapsed.</div>
            ) : (
              <div className="chat-shell">
                <div className="chat-meta-row">
                  <div className="chat-meta">
                    {chatStreaming
                      ? "Streaming response..."
                      : `${chatMessages.length} message${chatMessages.length === 1 ? "" : "s"}`}
                  </div>
                  <div className="actions" style={{ marginTop: 0 }}>
                    <button
                      className="secondary"
                      onClick={() => clearChat()}
                      disabled={chatMessages.length === 0 && !chatStreaming}
                    >
                      Clear Chat
                    </button>
                    {chatStreaming ? (
                      <button
                        className="secondary"
                        onClick={() => {
                          chatAbortRef.current?.abort();
                        }}
                      >
                        Stop
                      </button>
                    ) : null}
                  </div>
                </div>
                <div className="chat-history">
                  {chatMessages.length === 0 ? (
                    <div className="chat-placeholder">Send a prompt to start chat.</div>
                  ) : (
                    chatMessages.map((message) => (
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
                            [Applied {message.changeCount} file change
                            {message.changeCount === 1 ? "" : "s"}]
                            {message.canViewDiff ? (
                              <button className="secondary" onClick={() => openLatestDiff()}>
                                View Diff
                              </button>
                            ) : null}
                          </div>
                        ) : null}
                      </div>
                    ))
                  )}
                </div>
                <div className="chat-settings-grid">
                  <div className="field">
                    <label>Claude API Key</label>
                    <input
                      type="password"
                      value={claudeApiKey}
                      onChange={(event) => setClaudeApiKey(event.target.value)}
                      placeholder="sk-ant-..."
                      disabled={codeSettingsLoading || codeSettingsSaving}
                    />
                  </div>
                  <div className="field">
                    <label>OpenAI API Key</label>
                    <input
                      type="password"
                      value={openaiApiKey}
                      onChange={(event) => setOpenaiApiKey(event.target.value)}
                      placeholder="sk-..."
                      disabled={codeSettingsLoading || codeSettingsSaving}
                    />
                  </div>
                  <div className="field">
                    <label>Provider</label>
                    <select
                      value={llmProvider}
                      onChange={handleProviderSelect}
                      disabled={codeSettingsLoading || codeSettingsSaving}
                    >
                      <option value="claude">claude</option>
                      <option value="openai">openai</option>
                    </select>
                  </div>
                  <div className="field">
                    <label>Model</label>
                    <input
                      value={llmModel}
                      onChange={(event) => setLlmModel(event.target.value)}
                      placeholder="claude-sonnet-4-5-20250929"
                      disabled={codeSettingsLoading || codeSettingsSaving}
                    />
                  </div>
                </div>
                {chatError ? (
                  <div className="status err">
                    {chatError}
                    {!chatStreaming && lastChatPrompt ? (
                      <button
                        className="secondary"
                        style={{ marginLeft: "8px" }}
                        onClick={() => void sendChatMessage({ textOverride: lastChatPrompt })}
                      >
                        Retry
                      </button>
                    ) : null}
                  </div>
                ) : null}
                <div className="chat-input-row">
                  <textarea
                    value={chatInput}
                    placeholder="Type a message..."
                    onChange={(event) => setChatInput(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" && !event.shiftKey) {
                        event.preventDefault();
                        void sendChatMessage();
                      }
                    }}
                    disabled={chatStreaming || codeSettingsLoading || codeSettingsSaving}
                  />
                  <button
                    className="secondary"
                    onClick={() => void loadCodeSettings()}
                    disabled={codeSettingsLoading || codeSettingsSaving}
                  >
                    {codeSettingsLoading ? "Loading..." : "Reload"}
                  </button>
                  <button
                    className="primary"
                    onClick={() => void saveCodeSettings()}
                    disabled={codeSettingsLoading || codeSettingsSaving}
                  >
                    {codeSettingsSaving ? "Saving..." : "Save"}
                  </button>
                  <button
                    className="primary"
                    onClick={() => void sendChatMessage()}
                    disabled={
                      chatStreaming ||
                      codeSettingsLoading ||
                      codeSettingsSaving ||
                      chatInput.trim().length === 0
                    }
                  >
                    {chatStreaming ? "Sending..." : "Send"}
                  </button>
                </div>
              </div>
            )}
          </div>
        </div>

        {quickOpenVisible ? (
          <div
            className="quick-open-overlay"
            onMouseDown={(event) => {
              if (event.target === event.currentTarget) {
                closeQuickOpen();
              }
            }}
          >
            <div className="quick-open-modal" onMouseDown={(event) => event.stopPropagation()}>
              <input
                ref={quickOpenInputRef}
                value={quickOpenQuery}
                placeholder="Quick Open (Ctrl/Cmd+P)"
                onChange={(event) => {
                  setQuickOpenQuery(event.target.value);
                  setQuickOpenIndex(0);
                }}
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
                      onClick={() => {
                        void selectQuickOpenPath(filePath);
                      }}
                    >
                      {filePath}
                    </button>
                  ))
                )}
              </div>
            </div>
          </div>
        ) : null}

        {commandPaletteVisible ? (
          <div
            className="quick-open-overlay"
            onMouseDown={(event) => {
              if (event.target === event.currentTarget) {
                closeCommandPalette();
              }
            }}
          >
            <div className="quick-open-modal" onMouseDown={(event) => event.stopPropagation()}>
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

const rootEl = document.getElementById("root");
if (rootEl) {
  createRoot(rootEl).render(<App />);
}
