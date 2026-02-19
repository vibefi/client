import { jsonSchema, tool } from "ai";
import type { SendChatParams } from "./provider";
import {
  parseToolCallInput,
  type DeleteFileToolInput,
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
    path: {
      type: "string",
      description: "Relative file path to delete",
      minLength: 1,
    },
  },
  required: ["path"],
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
        if (!parsedInput || "content" in parsedInput) {
          throw new Error("Invalid delete_file tool input.");
        }

        return executeToolCall(
          params,
          {
            id: toolCallId,
            name: "delete_file",
            input: parsedInput,
          },
          toolResults,
        );
      },
    }),
  } as const;
}
