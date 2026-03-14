import React from "react";

type DiffViewerProps = {
  diffText: string;
};

function lineClassName(line: string): string {
  if (line.startsWith("+++ ") || line.startsWith("--- ")) return "meta";
  if (line.startsWith("+++") || line.startsWith("---")) return "meta";
  if (line.startsWith("@@")) return "hunk";
  if (line.startsWith("+")) return "add";
  if (line.startsWith("-")) return "remove";
  if (line.startsWith("── ")) return "file";
  return "base";
}

export function DiffViewer({ diffText }: DiffViewerProps) {
  const lines = diffText.split("\n");

  return (
    <pre className="diff-pre">
      {lines.map((line, index) => (
        <div className={`diff-line ${lineClassName(line)}`} key={`diff-${index}`}>
          {line || " "}
        </div>
      ))}
    </pre>
  );
}
