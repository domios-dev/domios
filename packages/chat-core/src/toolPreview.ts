const ANSI_ESCAPE = /\u001b\[[0-?]*[ -/]*[@-~]/g;

export interface ToolPreview {
  lines: string[];
  hiddenLines: number;
  totalLines: number;
}

export function buildToolPreview(text: string, maxLines = 2): ToolPreview {
  const normalizedMaxLines = Number.isFinite(maxLines) ? Math.max(0, Math.trunc(maxLines)) : 2;
  const stripped = text.replace(/\r/g, "").replace(ANSI_ESCAPE, "");
  const lines = stripped
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
  const tail = normalizedMaxLines > 0 ? lines.slice(-normalizedMaxLines) : [];
  const hiddenLines = Math.max(0, lines.length - tail.length);
  return { lines: tail, hiddenLines, totalLines: lines.length };
}
