import type { ToolCallCardData } from "./chat/ToolCallCard";
import { CHAT_TAB_ID, CONSOLE_TAB_ID, DIFF_TAB_ID } from "./constants";

export type ProjectSummary = {
  name: string;
  path: string;
  lastModified?: string | number;
};

export type FileEntry = {
  name: string;
  path: string;
  isDir: boolean;
  size?: number;
  children?: FileEntry[];
};

export type OpenProjectResult = {
  projectPath: string;
  files: FileEntry[];
};

export type DevServerStatus = {
  running: boolean;
  port: number | null;
};

export type FileTab = {
  id: string;
  kind: "file";
  path: string;
  content: string;
  savedContent: string;
  isLoading: boolean;
  isSaving: boolean;
};

export type ConsoleTab = {
  id: typeof CONSOLE_TAB_ID;
  kind: "console";
  title: string;
};

export type DiffTab = {
  id: typeof DIFF_TAB_ID;
  kind: "diff";
  title: string;
  diffText: string;
};

export type ChatTab = {
  id: typeof CHAT_TAB_ID;
  kind: "chat";
  title: string;
};

export type EditorTab = FileTab | ConsoleTab | DiffTab | ChatTab;

export type ConsolePathMatch = {
  path: string;
  line: number;
  start: number;
  end: number;
};

export type ChatUiMessage = {
  id: string;
  role: "user" | "assistant";
  content: string;
  toolCalls?: ToolCallCardData[];
  changeCount?: number;
  canViewDiff?: boolean;
};

export type WorkspaceMode = "llm-preview" | "llm-code-preview";

export type SidebarPanelId = "projects" | "files" | "dev-server" | "console";
