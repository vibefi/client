export type ToolName =
  | "read_file"
  | "write_file"
  | "delete_file"
  | "edit_file"
  | "grep_search"
  | "read_file_section";

export type ReadFileToolInput = { path: string };
export type WriteFileToolInput = { path: string; content: string };
export type DeleteFileToolInput = { path: string };
export type EditFileToolInput = { path: string; targetContent: string; replacementContent: string };
export type GrepSearchToolInput = { query: string };
export type ReadFileSectionToolInput = { path: string; startLine: number; endLine: number };

export type ToolInput =
  | ReadFileToolInput
  | WriteFileToolInput
  | DeleteFileToolInput
  | EditFileToolInput
  | GrepSearchToolInput
  | ReadFileSectionToolInput;

export type ToolCall = {
  id: string;
  name: ToolName;
  input: ToolInput;
};

export type ToolExecutionResult = {
  toolCallId: string;
  name: ToolName;
  ok: boolean;
  output: string;
};

export const CLAUDE_TOOL_SCHEMAS = [
  {
    name: "read_file",
    description: "Read a file from the project before editing it.",
    input_schema: {
      type: "object",
      properties: { path: { type: "string", description: "Relative file path to read" } },
      required: ["path"],
    },
  },
  {
    name: "write_file",
    description: "Create or overwrite a whole file in the project. Prefer edit_file for small changes.",
    input_schema: {
      type: "object",
      properties: {
        path: { type: "string" },
        content: { type: "string", description: "Full file content" },
      },
      required: ["path", "content"],
    },
  },
  {
    name: "delete_file",
    description: "Delete a file from the project.",
    input_schema: {
      type: "object",
      properties: { path: { type: "string" } },
      required: ["path"],
    },
  },
  {
    name: "edit_file",
    description: "Edit a file by finding exactly targetContent and replacing it with replacementContent. targetContent must be a unique substring of the file.",
    input_schema: {
      type: "object",
      properties: {
        path: { type: "string" },
        targetContent: { type: "string", description: "Exact string to replace. Must exactly match the existing file." },
        replacementContent: { type: "string", description: "The content to replace the target with." },
      },
      required: ["path", "targetContent", "replacementContent"],
    },
  },
  {
    name: "grep_search",
    description: "Search all files in the project for a query string.",
    input_schema: {
      type: "object",
      properties: { query: { type: "string", description: "Search query" } },
      required: ["query"],
    },
  },
  {
    name: "read_file_section",
    description: "Read a specific line range of a file.",
    input_schema: {
      type: "object",
      properties: {
        path: { type: "string" },
        startLine: { type: "number", description: "1-indexed start line" },
        endLine: { type: "number", description: "1-indexed end line" },
      },
      required: ["path", "startLine", "endLine"],
    },
  },
] as const;

export const OPENAI_TOOL_SCHEMAS = CLAUDE_TOOL_SCHEMAS.map(schema => ({
  type: "function" as const,
  function: {
    name: schema.name,
    description: schema.description,
    parameters: schema.input_schema,
  }
}));

function asToolName(value: unknown): ToolName | null {
  return typeof value === "string" && ["read_file", "write_file", "delete_file", "edit_file", "grep_search", "read_file_section"].includes(value)
    ? (value as ToolName)
    : null;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object") return null;
  return value as Record<string, unknown>;
}

export function parseToolCallInput(nameValue: unknown, inputValue: unknown): ToolInput | null {
  const name = asToolName(nameValue);
  const input = asRecord(inputValue);
  if (!name || !input) return null;

  if (name === "grep_search") {
    const query = typeof input.query === "string" ? input.query : "";
    return query ? { query } : null;
  }

  const path = typeof input.path === "string" ? input.path.trim() : "";
  if (!path) return null;

  if (name === "read_file" || name === "delete_file") return { path };

  if (name === "write_file") {
    const content = typeof input.content === "string" ? input.content : null;
    if (content === null) return null;
    return { path, content };
  }

  if (name === "edit_file") {
    const targetContent = typeof input.targetContent === "string" ? input.targetContent : null;
    const replacementContent = typeof input.replacementContent === "string" ? input.replacementContent : null;
    if (targetContent === null || replacementContent === null) return null;
    return { path, targetContent, replacementContent };
  }

  if (name === "read_file_section") {
    const startLine = typeof input.startLine === "number" ? input.startLine : null;
    const endLine = typeof input.endLine === "number" ? input.endLine : null;
    if (startLine === null || endLine === null) return null;
    return { path, startLine, endLine };
  }

  return null;
}

export function asSupportedToolName(nameValue: unknown): ToolName | null {
  return asToolName(nameValue);
}
