import type { SendChatParams, SendChatResult } from "./provider";
import {
  asSupportedToolName,
  OPENAI_TOOL_SCHEMAS,
  parseToolCallInput,
  type ToolCall,
  type ToolExecutionResult,
} from "./tools";

type OpenAiToolFunction = {
  name?: string;
  arguments?: string;
};

type OpenAiToolCall = {
  id?: string;
  type?: string;
  function?: OpenAiToolFunction;
};

type OpenAiMessage = {
  role: "system" | "user" | "assistant" | "tool";
  content: string | null;
  tool_calls?: OpenAiToolCall[];
  tool_call_id?: string;
  name?: string;
};

type OpenAiChoice = {
  message?: OpenAiMessage;
};

type OpenAiResponse = {
  choices?: OpenAiChoice[];
  error?: {
    message?: string;
  };
};

function asRecord(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object") return null;
  return value as Record<string, unknown>;
}

function pickOpenAiError(data: unknown): string | null {
  const record = asRecord(data);
  if (!record) return null;

  const error = asRecord(record.error);
  if (error && typeof error.message === "string" && error.message.trim()) {
    return error.message.trim();
  }

  return null;
}

async function parseOpenAiError(response: Response): Promise<string> {
  const bodyText = await response.text();
  if (!bodyText.trim()) {
    return `OpenAI request failed (${response.status})`;
  }

  try {
    const parsed = JSON.parse(bodyText) as unknown;
    const apiError = pickOpenAiError(parsed);
    if (apiError) {
      return apiError;
    }
  } catch {
    // Ignore JSON parse error and return raw text.
  }

  return `OpenAI request failed (${response.status}): ${bodyText.slice(0, 300)}`;
}

function extractToolCalls(message: OpenAiMessage | undefined): ToolCall[] {
  if (!message || !Array.isArray(message.tool_calls)) {
    return [];
  }

  const toolCalls: ToolCall[] = [];
  for (const toolCall of message.tool_calls) {
    if (toolCall.type !== "function") continue;
    const id = typeof toolCall.id === "string" && toolCall.id.trim() ? toolCall.id.trim() : "";
    const functionName = toolCall.function?.name;
    const name = asSupportedToolName(functionName);
    if (!id || !name) continue;

    let parsedArgs: unknown = null;
    try {
      parsedArgs = toolCall.function?.arguments ? JSON.parse(toolCall.function.arguments) : null;
    } catch {
      continue;
    }

    const input = parseToolCallInput(functionName, parsedArgs);
    if (!input) continue;

    toolCalls.push({
      id,
      name,
      input,
    });
  }
  return toolCalls;
}

export async function streamOpenAiChat(params: SendChatParams): Promise<SendChatResult> {
  const toolResults: ToolExecutionResult[] = [];
  const maxToolRounds = Math.max(1, Math.trunc(params.maxToolRounds ?? 8));
  const messages: OpenAiMessage[] = [];

  if (params.systemPrompt?.trim()) {
    messages.push({
      role: "system",
      content: params.systemPrompt,
    });
  }

  for (const message of params.messages) {
    messages.push({
      role: message.role,
      content: message.content,
    });
  }

  for (let round = 0; round < maxToolRounds; round += 1) {
    const response = await fetch("https://api.openai.com/v1/chat/completions", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: `Bearer ${params.apiKey}`,
      },
      body: JSON.stringify({
        model: params.model,
        messages,
        tools: OPENAI_TOOL_SCHEMAS,
        tool_choice: "auto",
      }),
      signal: params.signal,
    });

    if (!response.ok) {
      throw new Error(await parseOpenAiError(response));
    }

    const payload = (await response.json()) as OpenAiResponse;
    if (payload.error?.message) {
      throw new Error(payload.error.message);
    }

    const assistantMessage = payload.choices?.[0]?.message;
    if (!assistantMessage || assistantMessage.role !== "assistant") {
      throw new Error("OpenAI response did not include an assistant message.");
    }

    const text = typeof assistantMessage.content === "string" ? assistantMessage.content : "";
    if (text) {
      params.onDelta(text);
    }

    const toolCalls = extractToolCalls(assistantMessage);
    messages.push({
      role: "assistant",
      content: text || "",
      tool_calls: assistantMessage.tool_calls ?? [],
    });

    if (toolCalls.length === 0) {
      return { toolResults };
    }

    if (!params.onToolCall) {
      throw new Error("Assistant requested tool calls but no tool handler is configured.");
    }

    for (const toolCall of toolCalls) {
      const result = await params.onToolCall(toolCall);
      toolResults.push(result);
      params.onToolResult?.(result);
      messages.push({
        role: "tool",
        tool_call_id: toolCall.id,
        name: toolCall.name,
        content: result.output,
      });
    }
  }

  throw new Error("OpenAI tool loop exceeded max rounds");
}
