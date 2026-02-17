import { streamClaudeChat } from "./claude";
import { streamOpenAiChat } from "./openai";
import type { ToolCall, ToolExecutionResult } from "./tools";

export type ChatRole = "user" | "assistant";

export type ChatMessage = {
  role: ChatRole;
  content: string;
};

export type ChatProvider = "claude" | "openai";

export type SendChatParams = {
  provider: ChatProvider;
  model: string;
  apiKey: string;
  systemPrompt?: string;
  messages: ChatMessage[];
  signal?: AbortSignal;
  maxToolRounds?: number;
  onDelta: (text: string) => void;
  onToolCall?: (toolCall: ToolCall) => Promise<ToolExecutionResult>;
  onToolResult?: (result: ToolExecutionResult) => void;
};

export type SendChatResult = {
  toolResults: ToolExecutionResult[];
};

export async function sendChatStream(params: SendChatParams): Promise<SendChatResult> {
  if (params.provider === "claude") {
    return streamClaudeChat(params);
  }

  return streamOpenAiChat(params);
}
