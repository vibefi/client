import React, { useState } from "react";

export type ToolCallCardData = {
  id: string;
  name: "write_file" | "delete_file";
  path: string;
  content?: string;
  ok: boolean;
  output: string;
};

type ToolCallCardProps = {
  call: ToolCallCardData;
};

export function ToolCallCard({ call }: ToolCallCardProps) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className={`tool-call-card ${call.ok ? "ok" : "err"}`}>
      <button
        className="tool-call-toggle"
        onClick={() => setExpanded((value) => !value)}
        title={expanded ? "Collapse tool result" : "Expand tool result"}
      >
        <span>{expanded ? "[-]" : "[+]"}</span>
        <span>
          [{call.name}: {call.path}]
        </span>
      </button>
      <div className="tool-call-output">{call.output}</div>
      {expanded && call.name === "write_file" && typeof call.content === "string" ? (
        <pre className="tool-call-content">{call.content}</pre>
      ) : null}
    </div>
  );
}
