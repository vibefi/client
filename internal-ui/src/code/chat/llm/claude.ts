import type { SendChatParams, SendChatResult } from "./provider";
import {
  asSupportedToolName,
  CLAUDE_TOOL_SCHEMAS,
  parseToolCallInput,
  type ToolCall,
  type ToolExecutionResult,
} from "./tools";

type ClaudeToolUseBlock = {
  type: "tool_use";
  id?: string;
  name?: string;
  input?: unknown;
};

type ClaudeTextBlock = {
  type: "text";
  text?: string;
};

type ClaudeContentBlock = ClaudeToolUseBlock | ClaudeTextBlock | { type?: string; [key: string]: unknown };

type ClaudeMessage = {
  role: "user" | "assistant";
  content: string | Array<Record<string, unknown>>;
};

type ClaudeResponse = {
  content?: ClaudeContentBlock[];
  error?: {
    message?: string;
  };
};

function asRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object") return null;
  return value as Record<string, unknown>;
}

function pickClaudeError(data: unknown): string | null {
  const record = asRecord(data);
  if (!record) return null;

  const error = asRecord(record.error);
  if (error && typeof error.message === "string" && error.message.trim()) {
    return error.message.trim();
  }

  return null;
}

async function parseClaudeError(response: Response): Promise<string> {
  const bodyText = await response.text();
  if (!bodyText.trim()) {
    return `Claude request failed (${response.status})`;
  }

  try {
    const parsed = JSON.parse(bodyText) as unknown;
    const apiError = pickClaudeError(parsed);
    if (apiError) {
      return apiError;
    }
  } catch {
    // Ignore JSON parse error and return raw text.
  }

  return `Claude request failed (${response.status}): ${bodyText.slice(0, 300)}`;
}

function extractTextDelta(contentBlocks: ClaudeContentBlock[]): string {
  return contentBlocks
    .flatMap((block) => {
      if (block.type !== "text") return [];
      return typeof block.text === "string" && block.text ? [block.text] : [];
    })
    .join("");
}

function extractToolCalls(contentBlocks: ClaudeContentBlock[]): ToolCall[] {
  const toolCalls: ToolCall[] = [];

  for (const block of contentBlocks) {
    if (block.type !== "tool_use") continue;

    const name = asSupportedToolName(block.name);
    const input = parseToolCallInput(block.name, block.input);
    const id = typeof block.id === "string" && block.id.trim() ? block.id.trim() : "";
    if (!name || !input || !id) {
      continue;
    }

    toolCalls.push({
      id,
      name,
      input,
    });
  }

  return toolCalls;
}

function toClaudeToolResultBlock(result: ToolExecutionResult): Record<string, unknown> {
  return {
    type: "tool_result",
    tool_use_id: result.toolCallId,
    is_error: result.ok ? undefined : true,
    content: result.output,
  };
}

function toClaudeAssistantContent(contentBlocks: ClaudeContentBlock[]): Array<Record<string, unknown>> {
  return contentBlocks
    .map((block) => asRecord(block))
    .filter((block): block is Record<string, unknown> => Boolean(block));
}

export async function streamClaudeChat(params: SendChatParams): Promise<SendChatResult> {
  const toolResults: ToolExecutionResult[] = [];
  const maxToolRounds = Math.max(1, Math.trunc(params.maxToolRounds ?? 8));

  const messages: ClaudeMessage[] = params.messages.map((message) => ({
    role: message.role,
    content: message.content,
  }));

  for (let round = 0; round < maxToolRounds; round += 1) {
    const response = await fetch("https://api.anthropic.com/v1/messages", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-api-key": params.apiKey,
        "anthropic-version": "2023-06-01",
        "anthropic-dangerous-direct-browser-access": "true",
      },
      body: JSON.stringify({
        model: params.model,
        max_tokens: 4096,
        stream: false,
        system: params.systemPrompt ?? "",
        messages,
        tools: CLAUDE_TOOL_SCHEMAS,
      }),
      signal: params.signal,
    });

    if (!response.ok) {
      throw new Error(await parseClaudeError(response));
    }

    const payload = (await response.json()) as ClaudeResponse;
    if (payload.error?.message) {
      throw new Error(payload.error.message);
    }

    const contentBlocks = Array.isArray(payload.content) ? payload.content : [];
    const text = extractTextDelta(contentBlocks);
    if (text) {
      params.onDelta(text);
    }

    const toolCalls = extractToolCalls(contentBlocks);
    if (toolCalls.length === 0) {
      return { toolResults };
    }

    if (!params.onToolCall) {
      throw new Error("Assistant requested tool calls but no tool handler is configured.");
    }

    messages.push({
      role: "assistant",
      content: toClaudeAssistantContent(contentBlocks),
    });

    const toolResultBlocks: Array<Record<string, unknown>> = [];
    for (const toolCall of toolCalls) {
      const result = await params.onToolCall(toolCall);
      toolResults.push(result);
      params.onToolResult?.(result);
      toolResultBlocks.push(toClaudeToolResultBlock(result));
    }

    messages.push({
      role: "user",
      content: toolResultBlocks,
    });
  }

  throw new Error("Claude tool loop exceeded max rounds");
}
