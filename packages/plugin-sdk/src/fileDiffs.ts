export interface RecordedFileDiff {
  path: string;
  kind?: "add" | "modify" | "delete" | "rename";
  additions: number;
  deletions: number;
  binary?: boolean;
  truncated?: boolean;
  hunks: Array<{ oldStart: number; oldLines: number; newStart: number; newLines: number; lines: string[] }>;
}

function textLines(text: string): string[] {
  return text ? text.replace(/\n$/, "").split("\n") : [];
}

export function recordedTextDiff(path: unknown, oldText: unknown, newText: unknown): RecordedFileDiff[] {
  if (typeof path !== "string" || !path.trim() || typeof oldText !== "string" || typeof newText !== "string" || oldText === newText) return [];
  const old = textLines(oldText);
  const next = textLines(newText);
  return [{ path, additions: next.length, deletions: old.length, truncated: true,
    hunks: [{ oldStart: 1, oldLines: old.length, newStart: 1, newLines: next.length,
      lines: [...old.map(line => `-${line}`), ...next.map(line => `+${line}`)] }] }];
}

export function recordedPatchDiff(path: string, patch: string, kind: RecordedFileDiff["kind"]): RecordedFileDiff[] {
  if (!path.trim()) return [];
  const hunks: RecordedFileDiff["hunks"] = [];
  let current: RecordedFileDiff["hunks"][number] | undefined;
  for (const line of patch.replace(/\n$/, "").split("\n")) {
    const header = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/.exec(line);
    if (header) {
      current = { oldStart: Number(header[1]), oldLines: Number(header[2] ?? 1), newStart: Number(header[3]), newLines: Number(header[4] ?? 1), lines: [] };
      hunks.push(current);
    } else if (current) {
      if (!/^[ +\-\\]/.test(line)) return [];
      current.lines.push(line);
    }
  }
  if (hunks.some(h => h.lines.filter(l => /^[ -]/.test(l)).length !== h.oldLines
    || h.lines.filter(l => /^[ +]/.test(l)).length !== h.newLines)) return [];
  if (!hunks.length && kind === "modify") return [];
  const lines = hunks.flatMap(h => h.lines);
  return [{ path, kind, additions: lines.filter(l => l.startsWith("+")).length,
    deletions: lines.filter(l => l.startsWith("-")).length, hunks }];
}
