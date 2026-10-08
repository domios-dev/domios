import { timestampMs, type ChangesTurn, type FileDiff, type FileDiffHunk, type RecordedFileChange } from "@codeterm/chat-core";

export type ChangesScope = "all" | { turnId: string };
export type ChangeBarBlock = "add" | "del" | "none";

export const CHANGE_STATUS: Record<RecordedFileChange["kind"], { glyph: string; label: string }> = {
  add: { glyph: "A", label: "Added" },
  modify: { glyph: "M", label: "Modified" },
  delete: { glyph: "D", label: "Deleted" },
  rename: { glyph: "R", label: "Renamed" },
};

export function changeKey(file: RecordedFileChange): string {
  return file.movePath ?? file.path;
}

export function changesTotals(files: readonly RecordedFileChange[]) {
  return files.reduce((total, file) => ({ additions: total.additions + file.additions, deletions: total.deletions + file.deletions }), { additions: 0, deletions: 0 });
}

export function changesFilePath(file: RecordedFileChange, cwd?: string | null): string | null {
  const path = file.movePath ?? file.path;
  if (/^(?:[A-Za-z]:[\\/]|[\\/]|~[\\/])/.test(path)) return path;
  return cwd ? `${cwd.replace(/[\\/]$/, "")}/${path}` : null;
}

/** A fixed-width additions/deletions bar; a non-zero side always keeps at least one block. */
export function changeBar(additions: number, deletions: number, blocks = 5): ChangeBarBlock[] {
  const total = additions + deletions;
  if (total <= 0) return Array<ChangeBarBlock>(blocks).fill("none");
  let added = Math.round((blocks * additions) / total);
  if (additions > 0 && added === 0) added = 1;
  if (deletions > 0 && added === blocks) added = blocks - 1;
  return Array.from({ length: blocks }, (_, index) => index < added ? "add" : "del");
}

export function scopedFiles(changes: { turns: readonly ChangesTurn[]; cumulative: readonly RecordedFileChange[] }, scope: ChangesScope): readonly RecordedFileChange[] {
  if (scope === "all") return changes.cumulative;
  return changes.turns.find((turn) => turn.turnId === scope.turnId)?.files ?? [];
}

export interface TurnOption { turnId: string; number: number; label: string; detail: string }

/** Turns that recorded edits, newest first, numbered by their position in the whole transcript. */
export function turnOptions(turns: readonly ChangesTurn[], formatTime: (ms: number) => string): TurnOption[] {
  return turns.map((turn, index) => ({ turn, number: index + 1 })).filter(({ turn }) => turn.files.length > 0).reverse().map(({ turn, number }) => {
    const at = timestampMs(turn.at);
    const files = `${turn.files.length} ${turn.files.length === 1 ? "file" : "files"}`;
    return { turnId: turn.turnId, number, label: `Turn ${number}`, detail: at == null ? files : `${formatTime(at)}, ${files}` };
  });
}

export function turnNumber(turns: readonly ChangesTurn[], turnId: string): number | null {
  const index = turns.findIndex((turn) => turn.turnId === turnId);
  return index < 0 ? null : index + 1;
}

export function hunkHeader(hunk: FileDiffHunk): string {
  return `@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@`;
}

function diffText(diff: FileDiff): string {
  const before = diff.kind === "add" ? "/dev/null" : `a/${diff.path}`;
  const after = diff.kind === "delete" ? "/dev/null" : `b/${diff.movePath ?? diff.path}`;
  return [`--- ${before}`, `+++ ${after}`, ...diff.hunks.flatMap((hunk) => [hunkHeader(hunk), ...hunk.lines])].join("\n");
}

export function unifiedDiffText(file: RecordedFileChange): string {
  return file.diffs.map(diffText).join("\n");
}

/** A fence longer than any backtick run inside the body, so the block cannot close early. */
export function fenced(body: string, language = ""): string {
  const longest = Math.max(0, ...Array.from(body.matchAll(/`+/g), (match) => match[0].length));
  const fence = "`".repeat(Math.max(3, longest + 1));
  return `${fence}${language}\n${body}\n${fence}`;
}

export function hunkReference(path: string, hunk: FileDiffHunk): string {
  const end = hunk.newStart + Math.max(0, hunk.newLines - 1);
  const lines = hunk.newLines > 1 ? `${hunk.newStart}-${end}` : `${hunk.newStart}`;
  return `Recorded change in \`${path}:${lines}\`:\n${fenced([hunkHeader(hunk), ...hunk.lines].join("\n"), "diff")}`;
}

export function changeReference(path: string, file: RecordedFileChange, maxHunks = 3): string {
  const headers = file.diffs.flatMap((diff) => diff.hunks).map(hunkHeader);
  const listed = headers.slice(0, maxHunks).join(", ");
  const more = headers.length > maxHunks ? ` and ${headers.length - maxHunks} more` : "";
  const status = CHANGE_STATUS[file.kind].label.toLowerCase();
  return `Recorded change in \`${path}\` (${status}, +${file.additions} -${file.deletions}${listed ? `; ${listed}${more}` : ""})`;
}
