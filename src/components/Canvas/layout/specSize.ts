import type { Section } from "../types";
import type { CanvasLayoutSettings } from "../settings";

const CHARS_PER_LINE = 40;
const LINE_H = 19;

function bodyLines(text: string): number {
  const t = text.trim();
  if (!t) return 0;
  return t.split("\n").reduce((acc, line) => acc + Math.max(1, Math.ceil(line.length / CHARS_PER_LINE)), 0);
}

// Deterministic expanded-section height: header + wrapped body, clamped to a max
// (the card scrolls past that) so long bodies never overflow the board.
export function estimateSpecHeight(section: Section, s: CanvasLayoutSettings): number {
  const body = bodyLines(section.body) * LINE_H + (section.body.trim() ? 14 : 0);
  const h = s.specHeaderHeight + body + 8;
  return Math.max(s.specMinHeight, Math.min(s.specMaxHeight, h));
}
