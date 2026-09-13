export interface FileDiffHunk {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  lines: string[];
}

export interface FileDiff {
  path: string;
  movePath?: string;
  additions: number;
  deletions: number;
  hunks: FileDiffHunk[];
}

const LINE_MARKERS = " +-\\";
const HUNK_HEADER = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/;

function splitLines(text: string): string[] {
  const lines = text.split("\n");
  if (lines[lines.length - 1] === "") lines.pop();
  return lines;
}

function contentHunks(content: string, marker: "+" | "-"): FileDiffHunk[] {
  const lines = content ? splitLines(content) : [];
  if (lines.length === 0) return [];
  const added = marker === "+";
  return [{
    oldStart: added ? 0 : 1,
    oldLines: added ? 0 : lines.length,
    newStart: added ? 1 : 0,
    newLines: added ? lines.length : 0,
    lines: lines.map((line) => marker + line),
  }];
}

function unifiedHunks(diff: string): FileDiffHunk[] | null {
  const hunks: FileDiffHunk[] = [];
  let current: FileDiffHunk | null = null;
  for (const line of splitLines(diff)) {
    const header = HUNK_HEADER.exec(line);
    if (header) {
      current = {
        oldStart: Number(header[1]),
        oldLines: header[2] === undefined ? 1 : Number(header[2]),
        newStart: Number(header[3]),
        newLines: header[4] === undefined ? 1 : Number(header[4]),
        lines: [],
      };
      hunks.push(current);
    } else if (!current) {
      if (line.indexOf("--- ") !== 0 && line.indexOf("+++ ") !== 0) return null;
    } else if (!line || LINE_MARKERS.indexOf(line.charAt(0)) < 0) {
      return null;
    } else {
      current.lines.push(line);
    }
  }
  return hunks.length > 0 ? hunks : null;
}

function fileDiff(path: string, hunks: FileDiffHunk[], movePath: string): FileDiff {
  let additions = 0;
  let deletions = 0;
  for (const hunk of hunks) {
    for (const line of hunk.lines) {
      if (line.charAt(0) === "+") additions++;
      else if (line.charAt(0) === "-") deletions++;
    }
  }
  return movePath ? { path, movePath, additions, deletions, hunks } : { path, additions, deletions, hunks };
}

function changeDiff(path: string, value: unknown): FileDiff | null {
  if (!path.trim() || !value || typeof value !== "object" || Array.isArray(value)) return null;
  const change = value as { type?: unknown; content?: unknown; unified_diff?: unknown; move_path?: unknown };
  if ((change.type === "add" || change.type === "delete") && typeof change.content === "string") {
    return fileDiff(path, contentHunks(change.content, change.type === "add" ? "+" : "-"), "");
  }
  if (change.type !== "update" || typeof change.unified_diff !== "string") return null;
  const hunks = unifiedHunks(change.unified_diff);
  if (!hunks) return null;
  return fileDiff(path, hunks, typeof change.move_path === "string" && change.move_path.trim() ? change.move_path : "");
}

export function changedPaths(changes: unknown): string[] {
  if (!changes || typeof changes !== "object" || Array.isArray(changes)) return [];
  return Object.keys(changes).filter((path) => path.trim().length > 0);
}

/** One diff per changed file in a `changes` map; a malformed entry drops only its own file. */
export function codexFileDiffs(changes: unknown): FileDiff[] {
  const diffs: FileDiff[] = [];
  for (const path of changedPaths(changes)) {
    const diff = changeDiff(path, (changes as Record<string, unknown>)[path]);
    if (diff) diffs.push(diff);
  }
  return diffs;
}

/** Old/new line pairs per hunk, the shape apply_patch edits already give the ordinary edit row. */
export function diffEdits(diffs: FileDiff[]): { path: string; old: string; new: string }[] {
  const edits: { path: string; old: string; new: string }[] = [];
  for (const diff of diffs) {
    for (const hunk of diff.hunks) {
      const oldLines = hunk.lines.filter((line) => line.charAt(0) === "-").map((line) => line.substring(1));
      const newLines = hunk.lines.filter((line) => line.charAt(0) === "+").map((line) => line.substring(1));
      if (oldLines.length > 0 || newLines.length > 0) edits.push({ path: diff.path, old: oldLines.join("\n"), new: newLines.join("\n") });
    }
  }
  return edits;
}
