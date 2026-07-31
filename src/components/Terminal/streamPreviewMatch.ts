// Shared by the Chat UI streaming preview block and the offline streaming
// eval harness (testbench/streaming). One matcher, no drift.

const GUTTER_GLYPHS = /^[\s⏺●○◐◑⎿│┃║❯›→✻✳✢✶✷✺✽∗⋆*·└├▸▹]+/u;
const MARKUP_CHARS = /[*_`~#|>\\]/g;

/** Rendered terminal row → comparable text: gutter glyphs and markdown
 * syntax stripped, whitespace collapsed. */
export function normalizeStreamLine(line: string): string {
  return line
    .replace(GUTTER_GLYPHS, "")
    .replace(MARKUP_CHARS, "")
    .replace(/\s+/g, " ")
    .trim();
}

/** Canonical message contents → one searchable blob in the same normal form. */
export function canonicalBlob(contents: readonly string[]): string {
  return contents
    .map((content) => content.replace(MARKUP_CHARS, "").replace(/\s+/g, " ").trim())
    .filter((content) => content.length > 0)
    .join(" ");
}

const MIN_MATCH_CHARS = 4;

/**
 * Cut the longest preview prefix covered by canonical content; the uncovered
 * tail keeps streaming. Short/blank rows inherit the coverage of the last
 * decisive row so separators never block a cut.
 */
export function uncoveredPreviewTail(
  preview: readonly string[],
  blob: string,
): string[] {
  let cut = 0;
  let lastDecisive: boolean = true;
  for (let i = 0; i < preview.length; i++) {
    const normalized = normalizeStreamLine(preview[i]);
    const decisive = normalized.length >= MIN_MATCH_CHARS;
    const covered: boolean = decisive ? blob.includes(normalized) : lastDecisive;
    if (decisive) lastDecisive = covered;
    if (!covered) break;
    cut = i + 1;
  }
  return preview.slice(cut);
}
