export interface FileDiffHunk {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  lines: string[];
}

export interface FileDiff {
  path: string;
  additions: number;
  deletions: number;
  hunks: FileDiffHunk[];
}

const LINE_MARKERS = " +-\\";

function lineCount(value: unknown): number | null {
  return typeof value === "number" && Number.isInteger(value) && value >= 0 ? value : null;
}

function patchHunk(value: unknown): FileDiffHunk | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const hunk = value as Record<string, unknown>;
  const oldStart = lineCount(hunk.oldStart);
  const oldLines = lineCount(hunk.oldLines);
  const newStart = lineCount(hunk.newStart);
  const newLines = lineCount(hunk.newLines);
  if (oldStart === null || oldLines === null || newStart === null || newLines === null || !Array.isArray(hunk.lines)) return null;
  const lines: string[] = [];
  for (const line of hunk.lines) {
    if (typeof line !== "string" || !line || LINE_MARKERS.indexOf(line.charAt(0)) < 0) return null;
    lines.push(line);
  }
  return { oldStart, oldLines, newStart, newLines, lines };
}

// A created file carries an empty structuredPatch; its exact content is the whole added side.
function createdHunks(content: string): FileDiffHunk[] {
  if (!content) return [];
  const lines = content.split("\n");
  if (lines[lines.length - 1] === "") lines.pop();
  return [{ oldStart: 0, oldLines: 0, newStart: 1, newLines: lines.length, lines: lines.map((line) => "+" + line) }];
}

function fileDiff(path: string, hunks: FileDiffHunk[]): FileDiff {
  let additions = 0;
  let deletions = 0;
  for (const hunk of hunks) {
    for (const line of hunk.lines) {
      if (line.charAt(0) === "+") additions++;
      else if (line.charAt(0) === "-") deletions++;
    }
  }
  return { path, additions, deletions, hunks };
}

/** The per-file diff a write-tool result supplies; none when its patch is absent or malformed. */
export function claudeFileDiffs(result: unknown): FileDiff[] {
  if (!result || typeof result !== "object" || Array.isArray(result)) return [];
  const record = result as Record<string, unknown>;
  const path = typeof record.filePath === "string" ? record.filePath : "";
  if (!path.trim() || !Array.isArray(record.structuredPatch)) return [];
  const hunks: FileDiffHunk[] = [];
  for (const entry of record.structuredPatch) {
    const hunk = patchHunk(entry);
    if (!hunk) return [];
    hunks.push(hunk);
  }
  if (hunks.length > 0) return [fileDiff(path, hunks)];
  return record.type === "create" && typeof record.content === "string" ? [fileDiff(path, createdHunks(record.content))] : [];
}
