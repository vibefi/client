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
    "Only propose and generate code that is safe and minimal.",
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
