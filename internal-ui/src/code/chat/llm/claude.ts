import { stepCountIs, streamText } from "ai";
import { createAnthropic } from "@ai-sdk/anthropic";
import { buildFileTools } from "./aiSdkTools";
import type { SendChatParams, SendChatResult } from "./provider";
import type { ToolExecutionResult } from "./tools";
import { asErrorMessage } from "../../utils";

const STREAM_TIMEOUT_MS = 900_000;
const DEFAULT_MAX_TOOL_ROUNDS = 64;

function extractToolPath(input: unknown): string | null {
  if (!input || typeof input !== "object") return null;
  const pathValue = (input as Record<string, unknown>).path;
  if (typeof pathValue !== "string") return null;
  const path = pathValue.trim();
  return path || null;
}

export async function streamClaudeChat(params: SendChatParams): Promise<SendChatResult> {
  const toolResults: ToolExecutionResult[] = [];
  // Keep a high upper bound as a runaway-cost guard while allowing long tool loops.
  const maxToolRounds = Math.max(1, Math.trunc(params.maxToolRounds ?? DEFAULT_MAX_TOOL_ROUNDS));
  const chunkCounts: Record<string, number> = {};
  let stepCount = 0;
  let finishReason: string | undefined;
  let rawFinishReason: string | undefined;
  let aborted = false;
  let abortReason: string | undefined;
  let errorMessage: string | undefined;
  let usage: { inputTokens?: number; outputTokens?: number; totalTokens?: number } | undefined;

  const anthropic = createAnthropic({
    apiKey: params.apiKey,
    headers: {
      "anthropic-dangerous-direct-browser-access": "true",
    },
  });

  const result = streamText({
    model: anthropic.messages(params.model),
    system: params.systemPrompt?.trim() ? params.systemPrompt : undefined,
    messages: params.messages.map((message) => ({
      role: message.role,
      content: message.content,
    })),
    tools: buildFileTools(params, toolResults),
    stopWhen: stepCountIs(maxToolRounds),
    abortSignal: params.signal,
    timeout: STREAM_TIMEOUT_MS,
  });

  let lastStatus = "";
  const emitStatus = (status: string) => {
    if (!status || status === lastStatus) return;
    lastStatus = status;
    params.onStatus?.(status);
  };

  emitStatus("Connecting to Claude...");
  for await (const chunk of result.fullStream) {
    chunkCounts[chunk.type] = (chunkCounts[chunk.type] ?? 0) + 1;

    if (chunk.type === "start") {
      emitStatus("Thinking...");
      continue;
    }

    if (chunk.type === "start-step") {
      emitStatus("Planning next step...");
      continue;
    }

    if (chunk.type === "reasoning-start" || chunk.type === "reasoning-delta") {
      emitStatus("Analyzing request...");
      continue;
    }

    if (chunk.type === "tool-call") {
      const path = extractToolPath(chunk.input);
      emitStatus(path ? `Running ${chunk.toolName} on ${path}...` : `Running ${chunk.toolName}...`);
      continue;
    }

    if (chunk.type === "tool-result") {
      emitStatus(`Finished ${chunk.toolName}.`);
      continue;
    }

    if (chunk.type === "finish-step") {
      stepCount += 1;
      emitStatus("Processing step result...");
      continue;
    }

    if (chunk.type === "text-delta" && chunk.text) {
      params.onDelta(chunk.text);
      emitStatus("Writing response...");
      continue;
    }

    if (chunk.type === "finish") {
      finishReason = chunk.finishReason;
      rawFinishReason = chunk.rawFinishReason;
      usage = {
        inputTokens: chunk.totalUsage.inputTokens,
        outputTokens: chunk.totalUsage.outputTokens,
        totalTokens: chunk.totalUsage.totalTokens,
      };
      emitStatus("Done.");
      continue;
    }

    if (chunk.type === "abort") {
      aborted = true;
      abortReason = typeof chunk.reason === "string" ? chunk.reason : undefined;
      finishReason = finishReason ?? "abort";
      emitStatus("Request aborted.");
      continue;
    }

    if (chunk.type === "error") {
      errorMessage = asErrorMessage(chunk.error);
      finishReason = finishReason ?? "error";
      emitStatus("Model returned an error.");
    }
  }

  if (errorMessage) {
    throw new Error(`Chat stream error: ${errorMessage}`);
  }
  if (aborted) {
    throw new Error(`AbortError: ${abortReason ?? "stream aborted"}`);
  }

  return {
    toolResults,
    telemetry: {
      provider: params.provider,
      model: params.model,
      timeoutMs: STREAM_TIMEOUT_MS,
      maxToolRounds,
      steps: stepCount,
      finishReason,
      rawFinishReason,
      aborted,
      abortReason,
      errorMessage,
      usage,
      chunkCounts,
    },
  };
}
