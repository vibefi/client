import { type Extension } from "@codemirror/state";
import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import type { ChatProvider } from "./chat/llm/provider";
import type {
  DeleteFileToolInput,
  ReadFileToolInput,
  ToolCall,
  WriteFileToolInput,
} from "./chat/llm/tools";
import { CHAT_TAB_ID, CONSOLE_TAB_ID, DIFF_TAB_ID } from "./constants";
import type {
  ChatTab,
  ConsolePathMatch,
  ConsoleTab,
  DevServerStatus,
  DiffTab,
  EditorTab,
  FileEntry,
  FileTab,
  OpenProjectResult,
  ProjectSummary,
} from "./types";

export function asErrorMessage(error: unknown): string {
  if (error && typeof error === "object" && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string") return message;
  }
  return String(error);
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object";
}

function fallbackProjectName(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/").filter(Boolean);
  return parts[parts.length - 1] || path;
}

export function parseProjectsResult(value: unknown): ProjectSummary[] {
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

export function parseProjectPath(value: unknown, method: string): string {
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

export function parseOpenProjectResult(value: unknown): OpenProjectResult {
  const projectPath = parseProjectPath(value, "code_openProject");
  const files = isRecord(value) ? parseFileEntries(value.files) : [];
  return { projectPath, files };
}

export function parseListFilesResult(value: unknown): FileEntry[] {
  if (!isRecord(value)) return [];
  return parseFileEntries(value.files);
}

export function parseReadFileResult(value: unknown): string {
  if (isRecord(value) && typeof value.content === "string") {
    return value.content;
  }
  throw new Error("code_readFile returned invalid content");
}

export function parsePort(value: unknown): number | null {
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

export function parseDevServerStatus(value: unknown): DevServerStatus {
  if (!isRecord(value)) {
    return { running: false, port: null };
  }
  return {
    running: value.running === true,
    port: parsePort(value.port),
  };
}

export function asOptionalString(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

export function normalizeChatProvider(value: string | null | undefined): ChatProvider {
  const normalized = (value ?? "").trim().toLowerCase();
  if (normalized === "openai" || normalized === "chatgpt" || normalized === "gpt") {
    return "openai";
  }
  return "claude";
}

const OPENAI_MODEL_OPTIONS = ["gpt-5.2-codex"];
const CLAUDE_MODEL_OPTIONS = ["claude-sonnet-4-6", "claude-opus-4-6"];

export function modelOptionsForProvider(provider: ChatProvider): readonly string[] {
  return provider === "openai" ? OPENAI_MODEL_OPTIONS : CLAUDE_MODEL_OPTIONS;
}

export function defaultModelForProvider(provider: ChatProvider): string {
  return modelOptionsForProvider(provider)[0] ?? (provider === "openai" ? "gpt-5.3-codex" : "claude-sonnet-4-6");
}

export function normalizeModelForProvider(provider: ChatProvider, model: string | null | undefined): string {
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

export function formatLastModified(value: ProjectSummary["lastModified"]): string {
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

export function fileNameFromPath(path: string): string {
  const normalized = path.replace(/\\/g, "/").replace(/\/+$/, "");
  const parts = normalized.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? normalized;
}

export function tabIdForPath(filePath: string): string {
  return `file:${filePath}`;
}

export function createConsoleTab(): ConsoleTab {
  return { id: CONSOLE_TAB_ID, kind: "console", title: "Console" };
}

export function createDiffTab(diffText: string): DiffTab {
  return { id: DIFF_TAB_ID, kind: "diff", title: "Diff", diffText };
}

export function isFileTab(tab: EditorTab): tab is FileTab {
  return tab.kind === "file";
}

export function isDiffTab(tab: EditorTab): tab is DiffTab {
  return tab.kind === "diff";
}

export function isChatTab(tab: EditorTab): tab is ChatTab {
  return tab.kind === "chat";
}

export function createChatTab(): ChatTab {
  return { id: CHAT_TAB_ID, kind: "chat", title: "Chat" };
}

export function isFileTabDirty(tab: FileTab): boolean {
  return tab.content !== tab.savedContent;
}

export function chatMessageId(prefix: string): string {
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

export function isWriteFileInput(input: ToolCall["input"]): input is WriteFileToolInput {
  return "content" in input;
}

export function isReadFileInput(input: ToolCall["input"]): input is ReadFileToolInput {
  return !("content" in input);
}

export function isDeleteFileInput(input: ToolCall["input"]): input is DeleteFileToolInput {
  return !("content" in input);
}

export function parseConsolePathMatch(line: string): ConsolePathMatch | null {
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

export function normalizeConsolePathForProject(path: string, projectPath: string): string {
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

export function positionForLine(content: string, line: number): number {
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

export function flattenFilePaths(entries: FileEntry[]): string[] {
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

export function languageExtensionFromPath(filePath: string): Extension {
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
