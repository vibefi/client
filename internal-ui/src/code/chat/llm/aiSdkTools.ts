import { jsonSchema, tool } from "ai";
import type { SendChatParams } from "./provider";
import {
  parseToolCallInput,
  type DeleteFileToolInput,
  type EditFileToolInput,
  type GrepSearchToolInput,
  type ReadFileSectionToolInput,
  type ReadFileToolInput,
  type ToolCall,
  type ToolExecutionResult,
  type WriteFileToolInput,
} from "./tools";

const READ_FILE_INPUT_SCHEMA = jsonSchema<ReadFileToolInput>({
  type: "object",
  properties: {
    path: {
      type: "string",
      description: "Relative file path to read, e.g. src/app.css",
      minLength: 1,
    },
  },
  required: ["path"],
});

const WRITE_FILE_INPUT_SCHEMA = jsonSchema<WriteFileToolInput>({
  type: "object",
  properties: {
    path: {
      type: "string",
      description: "Relative file path, e.g. src/components/Table.tsx",
      minLength: 1,
    },
    content: {
      type: "string",
      description: "Full file content",
    },
  },
  required: ["path", "content"],
});

const DELETE_FILE_INPUT_SCHEMA = jsonSchema<DeleteFileToolInput>({
  type: "object",
  properties: {
    path: { type: "string", description: "Relative file path to delete", minLength: 1 },
  },
  required: ["path"],
});

const EDIT_FILE_INPUT_SCHEMA = jsonSchema<EditFileToolInput>({
  type: "object",
  properties: {
    path: { type: "string" },
    targetContent: { type: "string" },
    replacementContent: { type: "string" },
  },
  required: ["path", "targetContent", "replacementContent"],
});

const GREP_SEARCH_INPUT_SCHEMA = jsonSchema<GrepSearchToolInput>({
  type: "object",
  properties: { query: { type: "string" } },
  required: ["query"],
});

const READ_FILE_SECTION_INPUT_SCHEMA = jsonSchema<ReadFileSectionToolInput>({
  type: "object",
  properties: {
    path: { type: "string" },
    startLine: { type: "number" },
    endLine: { type: "number" },
  },
  required: ["path", "startLine", "endLine"],
});

function ensureToolHandler(params: SendChatParams): NonNullable<SendChatParams["onToolCall"]> {
  if (!params.onToolCall) {
    throw new Error("Assistant requested tool calls but no tool handler is configured.");
  }
  return params.onToolCall;
}

async function executeToolCall(
  params: SendChatParams,
  toolCall: ToolCall,
  toolResults: ToolExecutionResult[],
): Promise<string> {
  const onToolCall = ensureToolHandler(params);
  const result = await onToolCall(toolCall);
  toolResults.push(result);
  params.onToolResult?.(result);
  return result.output;
}

export function buildFileTools(params: SendChatParams, toolResults: ToolExecutionResult[]) {
  return {
    read_file: tool({
      description: "Read a file from the project before editing it.",
      inputSchema: READ_FILE_INPUT_SCHEMA,
      execute: async (input, { toolCallId }) => {
        const parsedInput = parseToolCallInput("read_file", input);
        if (!parsedInput || "content" in parsedInput) {
          throw new Error("Invalid read_file tool input.");
        }

        return executeToolCall(
          params,
          {
            id: toolCallId,
            name: "read_file",
            input: parsedInput,
          },
          toolResults,
        );
      },
    }),
    write_file: tool({
      description: "Create or overwrite a file in the project. Path is relative to project root.",
      inputSchema: WRITE_FILE_INPUT_SCHEMA,
      execute: async (input, { toolCallId }) => {
        const parsedInput = parseToolCallInput("write_file", input);
        if (!parsedInput || !("content" in parsedInput)) {
          throw new Error("Invalid write_file tool input.");
        }

        return executeToolCall(
          params,
          {
            id: toolCallId,
            name: "write_file",
            input: parsedInput,
          },
          toolResults,
        );
      },
    }),
    delete_file: tool({
      description: "Delete a file from the project.",
      inputSchema: DELETE_FILE_INPUT_SCHEMA,
      execute: async (input, { toolCallId }) => {
        const parsedInput = parseToolCallInput("delete_file", input);
        if (!parsedInput) throw new Error("Invalid delete_file tool input.");
        return executeToolCall(params, { id: toolCallId, name: "delete_file", input: parsedInput }, toolResults);
      },
    }),
    edit_file: tool({
      description: "Edit a file by finding exactly targetContent and replacing it with replacementContent.",
      inputSchema: EDIT_FILE_INPUT_SCHEMA,
      execute: async (input, { toolCallId }) => {
        const parsedInput = parseToolCallInput("edit_file", input);
        if (!parsedInput) throw new Error("Invalid edit_file tool input.");
        return executeToolCall(params, { id: toolCallId, name: "edit_file", input: parsedInput }, toolResults);
      },
    }),
    grep_search: tool({
      description: "Search all files in the project for a query.",
      inputSchema: GREP_SEARCH_INPUT_SCHEMA,
      execute: async (input, { toolCallId }) => {
        const parsedInput = parseToolCallInput("grep_search", input);
        if (!parsedInput) throw new Error("Invalid grep_search tool input.");
        return executeToolCall(params, { id: toolCallId, name: "grep_search", input: parsedInput }, toolResults);
      },
    }),
    read_file_section: tool({
      description: "Read a specific line range of a file.",
      inputSchema: READ_FILE_SECTION_INPUT_SCHEMA,
      execute: async (input, { toolCallId }) => {
        const parsedInput = parseToolCallInput("read_file_section", input);
        if (!parsedInput) throw new Error("Invalid read_file_section tool input.");
        return executeToolCall(params, { id: toolCallId, name: "read_file_section", input: parsedInput }, toolResults);
      },
    }),
  } as const;
}
