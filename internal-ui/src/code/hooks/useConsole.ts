import { useState } from "react";
import { MAX_CONSOLE_LINES } from "../constants";

export interface ConsoleHook {
  lines: string[];
  append: (lines: string[]) => void;
  clear: () => void;
}

export function useConsole(): ConsoleHook {
  const [lines, setLines] = useState<string[]>([]);

  function append(newLines: string[]) {
    if (newLines.length === 0) return;
    setLines((previous) => {
      const merged = [...previous, ...newLines];
      return merged.length > MAX_CONSOLE_LINES ? merged.slice(-MAX_CONSOLE_LINES) : merged;
    });
  }

  function clear() {
    setLines([]);
  }

  return { lines, append, clear };
}
