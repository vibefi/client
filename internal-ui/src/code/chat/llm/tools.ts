export type ToolName = "read_file" | "write_file" | "delete_file";

export type ReadFileToolInput = {
  path: string;
};

export type WriteFileToolInput = {
  path: string;
  content: string;
};

export type DeleteFileToolInput = {
  path: string;
};

export type ToolInput = ReadFileToolInput | WriteFileToolInput | DeleteFileToolInput;

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
      properties: {
        path: {
          type: "string",
          description: "Relative file path to read, e.g. src/app.css",
        },
      },
      required: ["path"],
    },
  },
  {
    name: "write_file",
    description: "Create or overwrite a file in the project. Path is relative to project root.",
    input_schema: {
      type: "object",
      properties: {
        path: {
          type: "string",
          description: "Relative file path, e.g. src/components/Table.tsx",
        },
        content: {
          type: "string",
          description: "Full file content",
        },
      },
      required: ["path", "content"],
    },
  },
  {
    name: "delete_file",
    description: "Delete a file from the project.",
    input_schema: {
      type: "object",
      properties: {
        path: {
          type: "string",
          description: "Relative file path to delete",
        },
      },
      required: ["path"],
    },
  },
] as const;

export const OPENAI_TOOL_SCHEMAS = [
  {
    type: "function",
    function: {
      name: "read_file",
      description: "Read a file from the project before editing it.",
      parameters: {
        type: "object",
        properties: {
          path: {
            type: "string",
            description: "Relative file path to read, e.g. src/app.css",
          },
        },
        required: ["path"],
      },
    },
  },
  {
    type: "function",
    function: {
      name: "write_file",
      description: "Create or overwrite a file in the project. Path is relative to project root.",
      parameters: {
        type: "object",
        properties: {
          path: {
            type: "string",
            description: "Relative file path, e.g. src/components/Table.tsx",
          },
          content: {
            type: "string",
            description: "Full file content",
          },
        },
        required: ["path", "content"],
      },
    },
  },
  {
    type: "function",
    function: {
      name: "delete_file",
      description: "Delete a file from the project.",
      parameters: {
        type: "object",
        properties: {
          path: {
            type: "string",
            description: "Relative file path to delete",
          },
        },
        required: ["path"],
      },
    },
  },
] as const;

function asToolName(value: unknown): ToolName | null {
  return value === "read_file" || value === "write_file" || value === "delete_file"
    ? value
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

  const path = typeof input.path === "string" ? input.path.trim() : "";
  if (!path) return null;

  if (name === "read_file" || name === "delete_file") {
    return { path };
  }

  const content = typeof input.content === "string" ? input.content : null;
  if (content === null) return null;

  return { path, content };
}

export function asSupportedToolName(nameValue: unknown): ToolName | null {
  return asToolName(nameValue);
}
