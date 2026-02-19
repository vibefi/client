import { stepCountIs, streamText } from "ai";
import { createAnthropic } from "@ai-sdk/anthropic";
import { buildFileTools } from "./aiSdkTools";
import type { SendChatParams, SendChatResult } from "./provider";
import type { ToolExecutionResult } from "./tools";

const STREAM_TIMEOUT_MS = 90_000;

function extractToolPath(input: unknown): string | null {
  if (!input || typeof input !== "object") return null;
  const pathValue = (input as Record<string, unknown>).path;
  if (typeof pathValue !== "string") return null;
  const path = pathValue.trim();
  return path || null;
}

export async function streamClaudeChat(params: SendChatParams): Promise<SendChatResult> {
  const toolResults: ToolExecutionResult[] = [];
  const maxToolRounds = Math.max(1, Math.trunc(params.maxToolRounds ?? 8));

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
      emitStatus("Processing step result...");
      continue;
    }

    if (chunk.type === "text-delta" && chunk.text) {
      params.onDelta(chunk.text);
      emitStatus("Writing response...");
      continue;
    }

    if (chunk.type === "finish") {
      emitStatus("Done.");
      continue;
    }

    if (chunk.type === "error") {
      emitStatus("Model returned an error.");
    }
  }

  return { toolResults };
}
