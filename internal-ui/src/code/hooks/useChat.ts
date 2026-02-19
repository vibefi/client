import { useEffect, useRef, useState } from "react";
import {
  sendChatStream,
  type ChatMessage as LlmChatMessage,
} from "../chat/llm/provider";
import { buildSystemPrompt } from "../chat/llm/system";
import type { ToolCall, ToolExecutionResult } from "../chat/llm/tools";
import { PROVIDER_IDS } from "../../ipc/contracts";
import type { IpcClient } from "../../ipc/client";
import type { DiffChange } from "../editor/diff";
import type { ChatUiMessage } from "../types";
import {
  asErrorMessage,
  chatMessageId,
  flattenFilePaths,
  isDeleteFileInput,
  isReadFileInput,
  isFileTab,
  isWriteFileInput,
  normalizeModelForProvider,
  parseReadFileResult,
} from "../utils";
import { DIFF_TAB_ID } from "../constants";
import type { SettingsHook } from "./useSettings";
import type { ProjectHook } from "./useProject";
import type { EditorHook } from "./useEditor";
import type { ConsoleHook } from "./useConsole";

export interface ChatHook {
  messages: ChatUiMessage[];
  messagesRef: React.MutableRefObject<ChatUiMessage[]>;
  input: string;
  streaming: boolean;
  streamStatus: string | null;
  error: string | null;
  lastPrompt: string;
  chatHistoryRef: React.RefObject<HTMLDivElement | null>;
  setInput: (v: string) => void;
  setError: (v: string | null) => void;
  send: (options?: { textOverride?: string }) => Promise<void>;
  clear: () => void;
  abort: () => void;
  abortRef: React.MutableRefObject<AbortController | null>;
}

export function useChat(
  client: IpcClient,
  settings: SettingsHook,
  project: ProjectHook,
  editor: EditorHook,
  console_: ConsoleHook
): ChatHook {
  const [messages, setMessages] = useState<ChatUiMessage[]>([]);
  const [input, setInput] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [streamStatus, setStreamStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [lastPrompt, setLastPrompt] = useState("");

  const messagesRef = useRef<ChatUiMessage[]>(messages);
  const abortRef = useRef<AbortController | null>(null);
  const chatHistoryRef = useRef<HTMLDivElement | null>(null);

  messagesRef.current = messages;

  // Auto-scroll chat history to bottom when messages change
  useEffect(() => {
    if (chatHistoryRef.current) {
      chatHistoryRef.current.scrollTop = chatHistoryRef.current.scrollHeight;
    }
  }, [messages]);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      abortRef.current?.abort();
    };
  }, []);

  function abort() {
    abortRef.current?.abort();
  }

  function clear() {
    abortRef.current?.abort();
    abortRef.current = null;
    setStreaming(false);
    setStreamStatus(null);
    setError(null);
    setMessages([]);
    editor.setLastChangeSet([]);
    editor.setPendingLineJump((current) => (current?.tabId === DIFF_TAB_ID ? null : current));
  }

  function appendDelta(messageId: string, chunk: string) {
    if (!chunk) return;
    setMessages((previous) =>
      previous.map((message) =>
        message.id === messageId
          ? { ...message, content: `${message.content}${chunk}` }
          : message
      )
    );
  }

  function normalizeToolPath(path: string): string {
    return path.trim().replace(/\\/g, "/").replace(/^\.\/+/, "");
  }

  function buildContextPrompt(): string {
    const openFileTabs = editor.openTabsRef.current.filter(isFileTab).map((tab) => ({
      path: tab.path,
      content: tab.content,
    }));
    return buildSystemPrompt({
      projectPath: project.activeProjectPathRef.current,
      filePaths: flattenFilePaths(project.fileTree),
      openFiles: openFileTabs,
    });
  }

  function mapToLlmMessages(uiMessages: ChatUiMessage[]): LlmChatMessage[] {
    return uiMessages.map((message) => ({
      role: message.role,
      content: message.content,
    }));
  }

  async function send(options: { textOverride?: string } = {}): Promise<void> {
    const text = (options.textOverride ?? input).trim();
    if (!text || streaming || settings.loading || settings.saving) return;

    const provider = settings.provider;
    const apiKey = (provider === "openai" ? settings.openaiApiKey : settings.claudeApiKey).trim();
    if (!apiKey) {
      setError(
        provider === "openai"
          ? "OpenAI API key is required to send chat messages."
          : "Claude API key is required to send chat messages."
      );
      return;
    }

    const model = normalizeModelForProvider(provider, settings.model);
    const userMessage: ChatUiMessage = {
      id: chatMessageId("user"),
      role: "user",
      content: text,
    };
    const assistantMessageId = chatMessageId("assistant");
    const assistantMessage: ChatUiMessage = {
      id: assistantMessageId,
      role: "assistant",
      content: "",
    };

    const toolChanges: DiffChange[] = [];
    const readFileCache = new Map<string, string>();
    const inspectedFiles = new Set(
      editor.openTabsRef.current
        .filter(isFileTab)
        .map((tab) => normalizeToolPath(tab.path))
        .filter((path) => path.length > 0)
    );
    const recordToolChange = (path: string, before: string | null, after: string | null) => {
      const existingIndex = toolChanges.findIndex((c) => c.path === path);
      if (existingIndex === -1) {
        toolChanges.push({
          path,
          before,
          after,
          kind: after === null ? "delete" : before === null ? "create" : "modify",
        });
        return;
      }
      const existing = toolChanges[existingIndex];
      const merged: DiffChange = {
        ...existing,
        after,
        kind: after === null ? "delete" : existing.before === null ? "create" : "modify",
      };
      const noNetChange =
        (merged.after === null && merged.before === null) ||
        (typeof merged.after === "string" &&
          typeof merged.before === "string" &&
          merged.after === merged.before);
      if (noNetChange) {
        toolChanges.splice(existingIndex, 1);
      } else {
        toolChanges[existingIndex] = merged;
      }
    };

    const nextMessages = [...messagesRef.current, userMessage];
    setMessages((previous) => [...previous, userMessage, assistantMessage]);
    if (!options.textOverride) setInput("");
    setError(null);
    setStreaming(true);
    setStreamStatus("Preparing request...");
    setLastPrompt(text);

    const controller = new AbortController();
    abortRef.current = controller;

    try {
      const result = await sendChatStream({
        provider,
        model,
        apiKey,
        systemPrompt: buildContextPrompt(),
        messages: mapToLlmMessages(nextMessages),
        signal: controller.signal,
        maxToolRounds: 8,
        onDelta: (chunk) => {
          appendDelta(assistantMessageId, chunk);
          setStreamStatus((current) =>
            current === "Writing response..." ? current : "Writing response..."
          );
        },
        onStatus: (status) => setStreamStatus(status),
        onToolCall: async (toolCall: ToolCall): Promise<ToolExecutionResult> => {
          const projectPath = project.activeProjectPathRef.current.trim();
          if (!projectPath) {
            const failed: ToolExecutionResult = {
              toolCallId: toolCall.id,
              name: toolCall.name,
              ok: false,
              output: "No active project is open.",
            };
            setMessages((previous) =>
              previous.map((message) =>
                message.id === assistantMessageId
                  ? {
                      ...message,
                      toolCalls: [
                        ...(message.toolCalls ?? []),
                        {
                          id: toolCall.id,
                          name: toolCall.name,
                          path: toolCall.input.path,
                          content: isWriteFileInput(toolCall.input)
                            ? toolCall.input.content
                            : undefined,
                          ok: false,
                          output: failed.output,
                        },
                      ],
                      changeCount: toolChanges.length,
                    }
                  : message
              )
            );
            return failed;
          }

          const targetPath = toolCall.input.path;
          const normalizedTargetPath = normalizeToolPath(targetPath);
          try {
            if (toolCall.name === "read_file" && isReadFileInput(toolCall.input)) {
              const cached = readFileCache.get(normalizedTargetPath);
              if (typeof cached === "string") {
                const success: ToolExecutionResult = {
                  toolCallId: toolCall.id,
                  name: toolCall.name,
                  ok: true,
                  output:
                    "File already read earlier in this turn. Reuse the previously returned contents and proceed with targeted edits.",
                };
                setMessages((previous) =>
                  previous.map((message) =>
                    message.id === assistantMessageId
                      ? {
                          ...message,
                          toolCalls: [
                            ...(message.toolCalls ?? []),
                            {
                              id: toolCall.id,
                              name: toolCall.name,
                              path: targetPath,
                              content:
                                cached.length > 600
                                  ? `${cached.slice(0, 600)}\n\n... [cached preview truncated]`
                                  : cached,
                              ok: true,
                              output: `Read ${targetPath} from cache (${cached.length} chars)`,
                            },
                          ],
                          changeCount: toolChanges.length,
                        }
                      : message
                  )
                );
                return success;
              }

              const result = await client.request(PROVIDER_IDS.code, "code_readFile", [
                { projectPath, filePath: targetPath },
              ]);
              const content = parseReadFileResult(result);
              readFileCache.set(normalizedTargetPath, content);
              inspectedFiles.add(normalizedTargetPath);
              const preview =
                content.length > 4000
                  ? `${content.slice(0, 4000)}\n\n... [truncated ${content.length - 4000} chars]`
                  : content;

              const success: ToolExecutionResult = {
                toolCallId: toolCall.id,
                name: toolCall.name,
                ok: true,
                output: content,
              };
              setMessages((previous) =>
                previous.map((message) =>
                  message.id === assistantMessageId
                    ? {
                        ...message,
                        toolCalls: [
                          ...(message.toolCalls ?? []),
                          {
                            id: toolCall.id,
                            name: toolCall.name,
                            path: targetPath,
                            content: preview,
                            ok: true,
                            output: `Read ${targetPath} (${content.length} chars)`,
                          },
                        ],
                        changeCount: toolChanges.length,
                      }
                    : message
                )
              );
              return success;
            }

            if (toolCall.name === "write_file" && isWriteFileInput(toolCall.input)) {
              const content = toolCall.input.content;
              const before = await editor.readFileSnapshot(projectPath, targetPath);
              if (before !== null && !inspectedFiles.has(normalizedTargetPath)) {
                const failed: ToolExecutionResult = {
                  toolCallId: toolCall.id,
                  name: toolCall.name,
                  ok: false,
                  output: `Refusing to overwrite existing file "${targetPath}" before it is read. Call read_file for this path first, then write_file with minimal targeted edits.`,
                };
                setMessages((previous) =>
                  previous.map((message) =>
                    message.id === assistantMessageId
                      ? {
                          ...message,
                          toolCalls: [
                            ...(message.toolCalls ?? []),
                            {
                              id: toolCall.id,
                              name: toolCall.name,
                              path: targetPath,
                              content,
                              ok: false,
                              output: failed.output,
                            },
                          ],
                          changeCount: toolChanges.length,
                        }
                      : message
                  )
                );
                return failed;
              }
              await client.request(PROVIDER_IDS.code, "code_writeFile", [
                { projectPath, filePath: targetPath, content },
              ]);
              inspectedFiles.add(normalizedTargetPath);
              project.ensureDirExpanded(targetPath);
              editor.replaceOpenFileTabContent(targetPath, content);
              recordToolChange(targetPath, before, content);

              const success: ToolExecutionResult = {
                toolCallId: toolCall.id,
                name: toolCall.name,
                ok: true,
                output: `Wrote ${targetPath}`,
              };
              setMessages((previous) =>
                previous.map((message) =>
                  message.id === assistantMessageId
                    ? {
                        ...message,
                        toolCalls: [
                          ...(message.toolCalls ?? []),
                          {
                            id: toolCall.id,
                            name: toolCall.name,
                            path: targetPath,
                            content,
                            ok: true,
                            output: success.output,
                          },
                        ],
                        changeCount: toolChanges.length,
                      }
                    : message
                )
              );
              return success;
            }

            if (toolCall.name === "delete_file" && isDeleteFileInput(toolCall.input)) {
              const before = await editor.readFileSnapshot(projectPath, targetPath);
              await client.request(PROVIDER_IDS.code, "code_deleteFile", [
                { projectPath, filePath: targetPath },
              ]);
              editor.closeOpenFileTab(targetPath);
              recordToolChange(targetPath, before, null);

              const success: ToolExecutionResult = {
                toolCallId: toolCall.id,
                name: toolCall.name,
                ok: true,
                output: `Deleted ${targetPath}`,
              };
              setMessages((previous) =>
                previous.map((message) =>
                  message.id === assistantMessageId
                    ? {
                        ...message,
                        toolCalls: [
                          ...(message.toolCalls ?? []),
                          {
                            id: toolCall.id,
                            name: toolCall.name,
                            path: targetPath,
                            ok: true,
                            output: success.output,
                          },
                        ],
                        changeCount: toolChanges.length,
                      }
                    : message
                )
              );
              return success;
            }

            throw new Error(`Unsupported tool call: ${toolCall.name}`);
          } catch (error) {
            const failed: ToolExecutionResult = {
              toolCallId: toolCall.id,
              name: toolCall.name,
              ok: false,
              output: asErrorMessage(error),
            };
            setMessages((previous) =>
              previous.map((message) =>
                message.id === assistantMessageId
                  ? {
                      ...message,
                      toolCalls: [
                        ...(message.toolCalls ?? []),
                        {
                          id: toolCall.id,
                          name: toolCall.name,
                          path: targetPath,
                          content: isWriteFileInput(toolCall.input)
                            ? toolCall.input.content
                            : undefined,
                          ok: false,
                          output: failed.output,
                        },
                      ],
                      changeCount: toolChanges.length,
                    }
                  : message
              )
            );
            return failed;
          }
        },
      });

      setMessages((previous) =>
        previous.map((message) =>
          message.id === assistantMessageId
            ? { ...message, changeCount: toolChanges.length, canViewDiff: toolChanges.length > 0 }
            : message
        )
      );

      const nextChangeSet = [...toolChanges];
      editor.setLastChangeSet(nextChangeSet);

      if (result.toolResults.length > 0) {
        await project.refreshFileTree(undefined, { silent: true });
      }
    } catch (error) {
      const message = asErrorMessage(error);
      const isAbort =
        (typeof DOMException !== "undefined" &&
          error instanceof DOMException &&
          error.name === "AbortError") ||
        message.includes("AbortError");
      if (!isAbort) {
        setError(message);
        setStreamStatus("Failed.");
        appendDelta(
          assistantMessageId,
          message ? `\n\n[error] ${message}` : "\n\n[error] Chat request failed"
        );
      } else {
        setError("Chat request canceled.");
        setStreamStatus("Canceled.");
        setMessages((previous) => {
          const target = previous.find((entry) => entry.id === assistantMessageId);
          if (!target) return previous;
          const hasVisibleContent =
            Boolean(target.content.trim()) || (target.toolCalls?.length ?? 0) > 0;
          if (hasVisibleContent) return previous;
          return previous.filter((entry) => entry.id !== assistantMessageId);
        });
      }
    } finally {
      abortRef.current = null;
      setStreaming(false);
      setStreamStatus(null);
    }
  }

  return {
    messages,
    messagesRef,
    input,
    streaming,
    streamStatus,
    error,
    lastPrompt,
    chatHistoryRef,
    setInput,
    setError,
    send,
    clear,
    abort,
    abortRef,
  };
}
