export type DiffChange = {
  path: string;
  kind: "create" | "modify" | "delete";
  before: string | null;
  after: string | null;
};

type DiffOp = {
  type: "equal" | "add" | "remove";
  line: string;
};

function toLines(value: string | null): string[] {
  if (typeof value !== "string") return [];
  const normalized = value.replace(/\r\n/g, "\n");
  if (normalized.length === 0) return [];
  const lines = normalized.split("\n");
  if (normalized.endsWith("\n")) {
    lines.pop();
  }
  return lines;
}

function buildLineOps(before: string[], after: string[]): DiffOp[] {
  const rows = before.length + 1;
  const cols = after.length + 1;
  const dp: number[][] = Array.from({ length: rows }, () => Array<number>(cols).fill(0));

  for (let i = before.length - 1; i >= 0; i -= 1) {
    for (let j = after.length - 1; j >= 0; j -= 1) {
      if (before[i] === after[j]) {
        dp[i][j] = dp[i + 1][j + 1] + 1;
      } else {
        dp[i][j] = Math.max(dp[i + 1][j], dp[i][j + 1]);
      }
    }
  }

  const ops: DiffOp[] = [];
  let i = 0;
  let j = 0;
  while (i < before.length && j < after.length) {
    if (before[i] === after[j]) {
      ops.push({ type: "equal", line: before[i] });
      i += 1;
      j += 1;
      continue;
    }

    if (dp[i + 1][j] >= dp[i][j + 1]) {
      ops.push({ type: "remove", line: before[i] });
      i += 1;
      continue;
    }

    ops.push({ type: "add", line: after[j] });
    j += 1;
  }

  while (i < before.length) {
    ops.push({ type: "remove", line: before[i] });
    i += 1;
  }
  while (j < after.length) {
    ops.push({ type: "add", line: after[j] });
    j += 1;
  }

  return ops;
}

function buildHunks(ops: DiffOp[], contextLines = 3): string[] {
  if (ops.length === 0) return [];

  const prefixOld = [0];
  const prefixNew = [0];
  for (const op of ops) {
    prefixOld.push(prefixOld[prefixOld.length - 1] + (op.type === "add" ? 0 : 1));
    prefixNew.push(prefixNew[prefixNew.length - 1] + (op.type === "remove" ? 0 : 1));
  }

  const hunks: string[] = [];
  let cursor = 0;
  while (cursor < ops.length) {
    while (cursor < ops.length && ops[cursor].type === "equal") {
      cursor += 1;
    }
    if (cursor >= ops.length) {
      break;
    }

    const start = Math.max(0, cursor - contextLines);
    let end = cursor;
    let trailingEqualCount = 0;

    while (end < ops.length) {
      if (ops[end].type === "equal") {
        trailingEqualCount += 1;
        if (trailingEqualCount > contextLines) {
          end -= contextLines;
          break;
        }
      } else {
        trailingEqualCount = 0;
      }
      end += 1;
    }

    if (end > ops.length) {
      end = ops.length;
    }

    const hunkOps = ops.slice(start, end);
    const oldCount = hunkOps.filter((op) => op.type !== "add").length;
    const newCount = hunkOps.filter((op) => op.type !== "remove").length;
    const oldStart = oldCount === 0 ? prefixOld[start] : prefixOld[start] + 1;
    const newStart = newCount === 0 ? prefixNew[start] : prefixNew[start] + 1;

    const hunkLines = [
      `@@ -${oldStart},${oldCount} +${newStart},${newCount} @@`,
      ...hunkOps.map((op) => {
        if (op.type === "add") return `+${op.line}`;
        if (op.type === "remove") return `-${op.line}`;
        return ` ${op.line}`;
      }),
    ];

    hunks.push(hunkLines.join("\n"));
    cursor = end;
  }

  return hunks;
}

function changeLabel(kind: DiffChange["kind"]): string {
  if (kind === "create") return "created";
  if (kind === "delete") return "deleted";
  return "modified";
}

function buildFileUnifiedDiff(change: DiffChange): string {
  const beforeLines = toLines(change.before);
  const afterLines = toLines(change.after);
  const ops = buildLineOps(beforeLines, afterLines);
  const hunks = buildHunks(ops);
  if (hunks.length === 0) {
    return "";
  }

  const oldPath = change.before === null ? "/dev/null" : `a/${change.path}`;
  const newPath = change.after === null ? "/dev/null" : `b/${change.path}`;

  return [`--- ${oldPath}`, `+++ ${newPath}`, ...hunks].join("\n");
}

export function buildUnifiedDiffForChanges(changes: DiffChange[]): string {
  if (changes.length === 0) {
    return "No file changes in the last LLM turn.";
  }

  return changes
    .map((change) => {
      const header = `── ${change.path} (${changeLabel(change.kind)}) ──`;
      const body = buildFileUnifiedDiff(change);
      return body ? `${header}\n${body}` : header;
    })
    .join("\n\n");
}
