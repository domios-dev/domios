import type { FileDiff, FileDiffHunk } from "./types";

const LINE_MARKERS = " +-\\";

function isCount(value: unknown): value is number {
  return typeof value === "number" && Number.isInteger(value) && value >= 0;
}

function isHunk(value: unknown): value is FileDiffHunk {
  if (!value || typeof value !== "object") return false;
  const hunk = value as Partial<Record<keyof FileDiffHunk, unknown>>;
  return isCount(hunk.oldStart) && isCount(hunk.oldLines) && isCount(hunk.newStart) && isCount(hunk.newLines)
    && Array.isArray(hunk.lines)
    && hunk.lines.every((line) => typeof line === "string" && line.length > 0 && LINE_MARKERS.includes(line.charAt(0)));
}

function isFileDiff(value: unknown): value is FileDiff {
  if (!value || typeof value !== "object") return false;
  const diff = value as Partial<Record<keyof FileDiff, unknown>>;
  if (typeof diff.path !== "string" || !diff.path.trim()) return false;
  if (diff.movePath !== undefined && typeof diff.movePath !== "string") return false;
  if (!isCount(diff.additions) || !isCount(diff.deletions) || !Array.isArray(diff.hunks) || !diff.hunks.every(isHunk)) return false;
  const lines = (diff.hunks as FileDiffHunk[]).flatMap((hunk) => hunk.lines);
  // Header counts must be the hunks' own; a count the detail cannot back is never shown.
  return diff.additions === lines.filter((line) => line.charAt(0) === "+").length
    && diff.deletions === lines.filter((line) => line.charAt(0) === "-").length;
}

/** Well-formed per-file diffs; keeps the supplied array's identity when every entry is valid. */
export function wellFormedFileDiffs(value: unknown): FileDiff[] | undefined {
  if (!Array.isArray(value)) return undefined;
  const valid = value.filter(isFileDiff);
  if (valid.length === 0) return undefined;
  return valid.length === value.length ? (value as FileDiff[]) : valid;
}
