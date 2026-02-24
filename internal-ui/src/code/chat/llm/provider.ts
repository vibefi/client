import { streamClaudeChat } from "./claude";
import { streamOpenAiChat } from "./openai";
import type { ToolCall, ToolExecutionResult } from "./tools";

export type ChatRole = "user" | "assistant";

export type ChatMessage = {
  role: ChatRole;
  content: string;
};

export type ChatProvider = "claude" | "openai";

export type ReasoningEffort = "low" | "medium" | "high";

export type SendChatParams = {
  provider: ChatProvider;
  model: string;
  apiKey: string;
  systemPrompt?: string;
  messages: ChatMessage[];
  signal?: AbortSignal;
  maxToolRounds?: number;
  reasoningEffort?: ReasoningEffort;
  onDelta: (text: string) => void;
  onStatus?: (status: string) => void;
  onToolCall?: (toolCall: ToolCall) => Promise<ToolExecutionResult>;
  onToolResult?: (result: ToolExecutionResult) => void;
};

export type ChatRunTelemetry = {
  provider: ChatProvider;
  model: string;
  timeoutMs: number;
  maxToolRounds: number;
  steps: number;
  finishReason?: string;
  rawFinishReason?: string;
  aborted?: boolean;
  abortReason?: string;
  errorMessage?: string;
  usage?: {
    inputTokens?: number;
    outputTokens?: number;
    totalTokens?: number;
  };
  chunkCounts?: Record<string, number>;
};

export type SendChatResult = {
  toolResults: ToolExecutionResult[];
  telemetry?: ChatRunTelemetry;
};

export async function sendChatStream(params: SendChatParams): Promise<SendChatResult> {
  if (params.provider === "claude") {
    return streamClaudeChat(params);
  }

  return streamOpenAiChat(params);
}
