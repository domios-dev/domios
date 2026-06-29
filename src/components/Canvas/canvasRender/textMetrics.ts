// One text-measuring source of truth so the footprint (layout, no live ctx) and
// drawCard (live ctx) wrap/ellipsize titles IDENTICALLY — the fix for title overflow
// is making sizing and rendering agree on metrics. measure() is a width fn: drawCard
// passes ctx.measureText; the footprint passes the offscreen-canvas measurer below.

export type Measure = (text: string) => number;

let offscreen: CanvasRenderingContext2D | null | undefined;
function offscreenCtx(): CanvasRenderingContext2D | null {
  if (offscreen !== undefined) return offscreen;
  try {
    offscreen = typeof document !== "undefined" ? document.createElement("canvas").getContext("2d") : null;
  } catch {
    offscreen = null;
  }
  return offscreen;
}

function fontPx(font: string): number {
  const m = /(\d+(?:\.\d+)?)px/.exec(font);
  return m ? Number(m[1]) : 13;
}

// Width measurer for a CSS font shorthand; headless fallback approximates glyph width.
export function measureFor(font: string): Measure {
  const ctx = offscreenCtx();
  if (ctx) {
    ctx.font = font;
    return (t) => ctx.measureText(t).width;
  }
  const approx = fontPx(font) * 0.55;
  return (t) => t.length * approx;
}

export function ellipsize(measure: Measure, text: string, maxW: number): string {
  if (measure(text) <= maxW) return text;
  let lo = 0;
  let hi = text.length;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (measure(text.slice(0, mid) + "…") <= maxW) lo = mid;
    else hi = mid - 1;
  }
  return lo > 0 ? text.slice(0, lo) + "…" : "…";
}

// Greedy word-wrap to at most `maxLines`; every line clamped to `maxW` with a real
// ellipsis (overflowing word, or the dumped remainder on the last line) so text NEVER
// exceeds the box in either axis.
export function wrapLines(measure: Measure, text: string, maxW: number, maxLines: number): string[] {
  const words = text.split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  const lines: string[] = [];
  let cur = "";
  for (let i = 0; i < words.length; i++) {
    const next = cur ? `${cur} ${words[i]}` : words[i];
    if (measure(next) <= maxW || !cur) {
      cur = next;
      continue;
    }
    lines.push(cur);
    if (lines.length === maxLines - 1) {
      cur = words.slice(i).join(" "); // last allowed line absorbs the remainder → ellipsized below
      break;
    }
    cur = words[i];
  }
  if (lines.length < maxLines) lines.push(cur);
  return lines.map((l) => ellipsize(measure, l, maxW));
}

export function lineCount(measure: Measure, text: string, maxW: number, maxLines: number): number {
  return Math.max(1, wrapLines(measure, text, maxW, maxLines).length);
}
