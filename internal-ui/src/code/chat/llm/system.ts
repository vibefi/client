export type SystemPromptInput = {
  projectPath: string;
  filePaths: string[];
  openFiles: Array<{ path: string; content: string }>;
};

export function buildSystemPrompt(input: SystemPromptInput): string {
  const fileList = input.filePaths.length > 0 ? input.filePaths.join("\n") : "(none)";
  const openFiles =
    input.openFiles.length > 0
      ? input.openFiles
          .map((file) => `# ${file.path}\n${file.content}`)
          .join("\n\n")
      : "(none)";

  return [
    "You are VibeFi Code, an AI assistant for building VibeFi dapps.",
    "",
    "Only propose and generate code that is safe, minimal, and reversible.",
    "Prefer surgical edits over full rewrites.",
    "",
    "Tool workflow requirements:",
    "- Before editing an existing file, read it first in this turn (unless it is already in Open File Buffers).",
    "- Never replace an entire file for a small change.",
    "- Preserve unrelated code, styles, comments, and formatting.",
    "- If the user asks for a focused tweak, change only the smallest relevant region.",
    "- After making edits, briefly verify in your response what was changed.",
    "",
    `Current Project Path: ${input.projectPath || "(not set)"}`,
    "",
    "Current Project Files:",
    fileList,
    "",
    "Open File Buffers:",
    openFiles,
  ].join("\n");
}
