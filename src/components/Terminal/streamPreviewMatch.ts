// Shared by the Chat UI streaming preview block and the offline streaming
// eval harness (testbench/streaming). One matcher, no drift.

const GUTTER_GLYPHS = /^[\s⏺●○◐◑⎿│┃║❯›→✻✳✢✶✷✺✽∗⋆*·•◦‣∙⁃└├▸▹-]+/u;
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

/** Rows the Chat streaming block renders at once; shared by the UI and the
 * offline simulator so both windows stay identical. */
export const MAX_PREVIEW_ROWS = 8;

/** Nested tool-output framing and collapse markers rendered by agent TUIs.
 * These rows are terminal chrome, never message content: canonical tool cards
 * already render the real payload. */
const JUNK_ROW = /^\s*(?:[│┃║└├⎿…]|\+?\d+\s+lines?\b)/u;

/** Live progress/status footer ("Working (1m 01s • esc to interrupt)"): ticks
 * every second and flaps in and out of the decoder's live view. */
const INTERRUPT_HINT = /(?:esc|ctrl\s*\+\s*c)\s+to\s+interrupt/i;

/** The TUI's own input-prompt echo of the user's draft/submitted text. */
const COMPOSER_ECHO = /^\s*[❯›]\s/u;

/** Horizontal rules and box-drawing separators (composer border, dividers). */
const RULE_ROW = /^[\s─-╿—―]+$/u;

/** Composer footer: "model · ~/path" / "model · /path" status line. */
const MODEL_CWD_FOOTER = /·\s*~?\/[\w./-]/u;

/** Unframed tool-output panes: diff rows (line number + -/+ marker or bare
 * line number, or line number + wide gutter) and CLI help option rows. */
const DIFF_ROW = /^\s{2,}\d+(?:\s*$|\s+[-+]|\s{2,}\S)/u;
const CLI_HELP_ROW = /^\s{2,}-{1,2}[A-Za-z][\w-]*[,\s]/u;

/** TUI status-badge rows ("▣  Build · <model>") — badge glyphs never gutter
 * message content — and reasoning-duration rows ("+ Thought: 304ms"). */
const STATUS_BADGE = /^\s*[▣▢◈◉]\s/u;
const DURATION_ROW = /^\s*\+?\s*\p{L}[\p{L} ]*:\s*\d+(?:\.\d+)?\s*m?s\s*$/u;

export function isPreviewJunkRow(line: string): boolean {
  return (
    line.trim().length === 0 ||
    JUNK_ROW.test(line) ||
    RULE_ROW.test(line) ||
    INTERRUPT_HINT.test(line) ||
    COMPOSER_ECHO.test(line) ||
    MODEL_CWD_FOOTER.test(line) ||
    DIFF_ROW.test(line) ||
    CLI_HELP_ROW.test(line) ||
    STATUS_BADGE.test(line) ||
    DURATION_ROW.test(line)
  );
}

/** Rows worth previewing as message content: junk frames dropped. */
export function displayablePreview(lines: readonly string[]): string[] {
  return lines.filter((line) => !isPreviewJunkRow(line));
}

/**
 * Cut everything up to and through the DEEPEST preview row covered by
 * canonical content; the tail below it keeps streaming. Uncovered rows above
 * a covered one (stray tool output, unfiltered chrome) die with the cut —
 * a single junk row must never pin already-landed content as a duplicate.
 * Short/blank rows after the cut are consumed so separators never linger.
 */
export function uncoveredPreviewTail(
  preview: readonly string[],
  blob: string,
): string[] {
  const normalized = preview.map(normalizeStreamLine);
  let deepestCovered = -1;
  if (blob.length > 0) {
    for (let i = preview.length - 1; i >= 0; i--) {
      if (normalized[i].length >= MIN_MATCH_CHARS && blob.includes(normalized[i])) {
        deepestCovered = i;
        break;
      }
    }
  }
  // Trailing separators die with the cut, but only after a REAL cut: with
  // nothing covered, a short row is fresh content (ultra-short answers must
  // still preview), never an inherited-coverage casualty.
  let cut = deepestCovered + 1;
  while (deepestCovered >= 0 && cut < preview.length && normalized[cut].length < MIN_MATCH_CHARS) {
    cut++;
  }
  return preview.slice(cut);
}
