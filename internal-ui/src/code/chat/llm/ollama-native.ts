/**
 * Ollama adapter using the native `/api/chat` endpoint via the `ollama` npm
 * package.  Unlike the OpenAI-compatible `/v1/chat/completions` endpoint,
 * the native API actually streams when tools are enabled — which avoids
 * WebKit's ~60s idle-connection timeout that kills non-streaming responses.
 */
import { Ollama } from "ollama/browser";
import type {
  ChatRequest as OllamaChatRequest,
  Message,
  Tool,
  ToolCall as OllamaToolCall,
} from "ollama/browser";
import { parseToolCallInput, type ToolCall, type ToolExecutionResult } from "./tools";
import type { SendChatParams, SendChatResult } from "./provider";
import { asErrorMessage } from "../../utils";

const DEFAULT_MAX_TOOL_ROUNDS = 64;
const DEFAULT_HEARTBEAT_MS = 10_000;

const OLLAMA_TOOLS: Tool[] = [
  {
    type: "function",
    function: {
      name: "read_file",
      description: "Read a file from the project before editing it.",
      parameters: {
        type: "object",
        required: ["path"],
        properties: {
          path: { type: "string", description: "Relative file path to read, e.g. src/app.css" },
        },
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
        required: ["path", "content"],
        properties: {
          path: { type: "string", description: "Relative file path, e.g. src/components/Table.tsx" },
          content: { type: "string", description: "Full file content" },
        },
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
        required: ["path"],
        properties: {
          path: { type: "string", description: "Relative file path to delete" },
        },
      },
    },
  },
];

function buildMessages(params: SendChatParams): Message[] {
  const msgs: Message[] = [];
  if (params.systemPrompt?.trim()) {
    msgs.push({ role: "system", content: params.systemPrompt });
  }
  for (const m of params.messages) {
    msgs.push({ role: m.role, content: m.content });
  }
  return msgs;
}

async function executeToolCalls(
  toolCalls: OllamaToolCall[],
  params: SendChatParams,
  toolResults: ToolExecutionResult[],
): Promise<Message[]> {
  const toolMessages: Message[] = [];
  for (const tc of toolCalls) {
    const name = tc.function.name;
    const args = tc.function.arguments;
    const parsed = parseToolCallInput(name, args);
    if (!parsed) {
      toolMessages.push({ role: "tool", content: `Error: invalid tool input for ${name}` });
      continue;
    }
    const call: ToolCall = { id: `tool-${Date.now()}`, name: name as ToolCall["name"], input: parsed };
    params.onStatus?.(`Running ${name}${("path" in parsed && parsed.path) ? ` on ${parsed.path}` : ""}...`);
    if (!params.onToolCall) {
      toolMessages.push({ role: "tool", content: "Error: no tool handler configured" });
      continue;
    }
    const result = await params.onToolCall(call);
    toolResults.push(result);
    params.onToolResult?.(result);
    toolMessages.push({ role: "tool", content: result.output });
  }
  return toolMessages;
}

export async function streamOllamaChat(params: SendChatParams): Promise<SendChatResult> {
  const toolResults: ToolExecutionResult[] = [];
  const maxToolRounds = Math.max(1, Math.trunc(params.maxToolRounds ?? DEFAULT_MAX_TOOL_ROUNDS));

  // Extract host from baseURL (strip /v1 suffix)
  const host = (params.baseURL ?? "http://localhost:11434").replace(/\/v1\/?$/, "");
  const client = new Ollama({ host });

  let stepCount = 0;
  let finishReason: string | undefined;
  let aborted = false;
  let abortReason: string | undefined;
  let errorMessage: string | undefined;
  const t0 = Date.now();
  let chunkCount = 0;
  let nonEmptyContentChunkCount = 0;
  let firstChunkAtMs: number | undefined;
  let lastChunkAtMs: number | undefined;
  let activeRound = 0;
  let activeRoundStartedAtMs: number | undefined;
  let activeRoundChunkCount = 0;
  let activeRoundFirstChunkAtMs: number | undefined;

  const messages = buildMessages(params);

  try {
    for (let round = 0; round < maxToolRounds; round++) {
      activeRound = round + 1;
      activeRoundStartedAtMs = Date.now() - t0;
      activeRoundChunkCount = 0;
      activeRoundFirstChunkAtMs = undefined;

      if (params.signal?.aborted) {
        aborted = true;
        abortReason = "signal aborted";
        break;
      }

      params.onStatus?.(round === 0 ? "Connecting to Ollama..." : "Processing tool results...");

      const request: (OllamaChatRequest & { stream: true }) & {
        stream_options: { heartbeat_ms: number };
      } = {
        model: params.model,
        messages,
        tools: OLLAMA_TOOLS,
        stream: true,
        stream_options: { heartbeat_ms: DEFAULT_HEARTBEAT_MS },
      };
      const stream = await client.chat(request);

      let assistantContent = "";
      let assistantToolCalls: OllamaToolCall[] | undefined;

      for await (const chunk of stream) {
        const nowMs = Date.now();
        chunkCount += 1;
        activeRoundChunkCount += 1;
        if (firstChunkAtMs === undefined) firstChunkAtMs = nowMs - t0;
        if (activeRoundFirstChunkAtMs === undefined) activeRoundFirstChunkAtMs = nowMs - t0;
        lastChunkAtMs = nowMs;

        if (params.signal?.aborted) {
          aborted = true;
          abortReason = "signal aborted";
          stream.abort();
          break;
        }

        if (chunk.message.content) {
          nonEmptyContentChunkCount += 1;
          assistantContent += chunk.message.content;
          params.onDelta(chunk.message.content);
          params.onStatus?.("Writing response...");
        }

        if (chunk.message.tool_calls?.length) {
          assistantToolCalls = chunk.message.tool_calls;
        }

        if (chunk.done) {
          finishReason = chunk.done_reason || "stop";
        }
      }

      stepCount++;

      if (aborted) break;

      // If model made tool calls, execute them and loop
      if (assistantToolCalls?.length) {
        // Add assistant message with tool calls to history
        messages.push({
          role: "assistant",
          content: assistantContent,
          tool_calls: assistantToolCalls,
        });

        const toolMsgs = await executeToolCalls(assistantToolCalls, params, toolResults);
        messages.push(...toolMsgs);
        continue;
      }

      // No tool calls — we're done
      finishReason = finishReason ?? "stop";
      break;
    }
  } catch (err) {
    if (params.signal?.aborted) {
      aborted = true;
      abortReason = "signal aborted";
    } else {
      const baseError = asErrorMessage(err);
      const errorAtMs = Date.now() - t0;
      const sinceLastChunkMs =
        typeof lastChunkAtMs === "number" ? Math.max(0, Date.now() - lastChunkAtMs) : -1;
      const lastChunkMs = typeof lastChunkAtMs === "number" ? lastChunkAtMs - t0 : -1;
      const activeRoundAgeMs =
        typeof activeRoundStartedAtMs === "number" ? Math.max(0, errorAtMs - activeRoundStartedAtMs) : -1;
      errorMessage = `${baseError} [ollama_stream_debug chunks=${chunkCount} nonEmptyContentChunks=${nonEmptyContentChunkCount} firstChunkMs=${firstChunkAtMs ?? -1} lastChunkMs=${lastChunkMs} errorAtMs=${errorAtMs} sinceLastChunkMs=${sinceLastChunkMs} activeRound=${activeRound} activeRoundStartMs=${activeRoundStartedAtMs ?? -1} activeRoundAgeMs=${activeRoundAgeMs} activeRoundChunks=${activeRoundChunkCount} activeRoundFirstChunkMs=${activeRoundFirstChunkAtMs ?? -1}]`;
    }
  }

  const durationMs = Date.now() - t0;
  params.onStatus?.(aborted ? "Request aborted." : errorMessage ? "Error." : "Done.");

  if (errorMessage) {
    throw new Error(`Chat stream error: ${errorMessage}`);
  }
  if (aborted) {
    throw new Error(`AbortError: ${abortReason ?? "stream aborted"}`);
  }

  return {
    toolResults,
    telemetry: {
      provider: "ollama",
      model: params.model,
      timeoutMs: durationMs,
      maxToolRounds,
      steps: stepCount,
      finishReason,
      aborted,
      abortReason,
      errorMessage,
      chunkCounts: {
        ollama_chunks: chunkCount,
        ollama_nonempty_content_chunks: nonEmptyContentChunkCount,
      },
    },
  };
}
