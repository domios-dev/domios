import { markdownToPlainText } from "../../lib/markdownText";

const PREVIEW_MAX_CHARS = 560;

function skipBlankLines(lines: string[], start: number): number {
  let cursor = start;
  while (cursor < lines.length && lines[cursor].trim() === "") cursor += 1;
  return cursor;
}

function copiedCodeToolbarEnd(lines: string[], start: number): number | null {
  if (lines[start]?.trim() !== "Code") return null;
  const language = lines[start + 1]?.trim() ?? "";
  if (!/^[a-z0-9_+#.-]+$/i.test(language)) return null;
  if (lines[start + 2]?.trim() !== "·") return null;
  if (!/^\d+ lines?$/.test(lines[start + 3]?.trim() ?? "")) return null;

  let cursor = skipBlankLines(lines, start + 4);
  if (["Show all", "Hide"].includes(lines[cursor]?.trim() ?? "")) {
    cursor = skipBlankLines(lines, cursor + 1);
  }
  if (!["Copy", "Copied"].includes(lines[cursor]?.trim() ?? "")) return null;
  cursor = skipBlankLines(lines, cursor + 1);
  if (lines[cursor]?.trim() !== "Fullscreen") return null;
  return cursor + 1;
}

function stripCopiedCodeToolbars(text: string): string {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const kept: string[] = [];
  for (let index = 0; index < lines.length;) {
    const end = copiedCodeToolbarEnd(lines, index);
    if (end == null) {
      kept.push(lines[index]);
      index += 1;
    } else {
      index = end;
    }
  }
  return kept.join("\n");
}

function truncatePreview(text: string): string {
  if (text.length <= PREVIEW_MAX_CHARS) return text;
  const candidate = text.slice(0, PREVIEW_MAX_CHARS - 1);
  const lastBoundary = Math.max(candidate.lastIndexOf(" "), candidate.lastIndexOf("\n"));
  const end = lastBoundary >= PREVIEW_MAX_CHARS * 0.75 ? lastBoundary : candidate.length;
  return `${candidate.slice(0, end).trimEnd()}…`;
}

export function queuedMessagePreview(text: string): string {
  const source = stripCopiedCodeToolbars(text);
  return truncatePreview(markdownToPlainText(source));
}
