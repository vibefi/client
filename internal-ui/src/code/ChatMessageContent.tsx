import React from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { ChatUiMessage } from "./types";

type ChatMessageContentProps = {
  message: ChatUiMessage;
};

export function ChatMessageContent({ message }: ChatMessageContentProps) {
  const content = message.content || (message.role === "assistant" ? "..." : "");
  if (message.role !== "assistant") {
    return <>{content}</>;
  }

  return (
    <ReactMarkdown remarkPlugins={[remarkGfm]} skipHtml>
      {content}
    </ReactMarkdown>
  );
}
