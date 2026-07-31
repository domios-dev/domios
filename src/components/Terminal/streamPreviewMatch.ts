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

/** Live progress/status footer ("Working (1m 01s • esc to interrupt)") and
 * keyboard-hint rows ("esc again to edit previous message"): tick/flap chrome. */
const INTERRUPT_HINT = /(?:esc|ctrl\s*\+\s*c)\s+(?:to\s+)?interrupt/i;
const KEY_HINT_ROW = /^\s*esc\b.*\bto\s+\p{L}+/iu;

/** Progress-dot status bars ("⬝⬝⬝⬝■■  esc interrupt … ctrl+p commands"). */
const PROGRESS_DOTS = /^\s*[⬝▪▫■□]{2,}/u;

/** Column-aligned status/footer rows (cwd path … token count … key hints):
 * an internal run of 6+ spaces never occurs in wrapped message prose. */
const MULTI_COLUMN_ROW = /\S\s{6,}\S/u;

/** The TUI's own input-prompt echo of the user's draft/submitted text.
 * Interactive-question OPTION rows ("❯ 1. Yes") reuse the same glyph but are
 * genuine assistant content the user must see — never filtered. */
const COMPOSER_ECHO = /^\s*[❯›]\s/u;
const OPTION_ROW = /^\s*[❯›]\s*\d+[.)]/u;

/** Horizontal rules, box-drawing and block-element separators (composer
 * borders, dividers, progress bars). */
const RULE_ROW = /^[\s─-▟—―]+$/u;

/** Braille-spinner status rows ("⠋ Thinking"). */
const SPINNER_ROW = /^\s*[⠁-⣿]\s/u;

/** Queued-prompt footer: "↳ <queued prompt>" plus its fully-parenthesized
 * hint row ("(… immediately)") pinned below the live region — including the
 * soft-wrapped tail of that hint ("immediately)": closes a paren it never
 * opened, with nothing after). */
const QUEUE_ROW = /^\s*↳/u;
const PAREN_FOOTER = /^\s*\([^()]*\)\s*$/u;
const PAREN_FRAGMENT = /^\s*[^()]*\)\s*$/u;

/** Session-start system banners: "Tip: …" and the usage-limit notice. */
const TIP_BANNER = /^\s*Tip:\s/iu;
const USAGE_BANNER = /\busage limit reset\b/iu;

/** Composer footer: "model · ~/path" / "model · /path" status line. */
const MODEL_CWD_FOOTER = /·\s*~?\/[\w./-]/u;

/** Unframed tool-output panes: diff rows (line number + -/+ marker or bare
 * line number, or line number + wide gutter) and CLI help option rows. */
const DIFF_ROW = /^\s{2,}\d+(?:\s*$|\s+[-+]|\s{2,}\S)/u;
const CLI_HELP_ROW = /^\s{2,}-{1,2}[A-Za-z][\w-]*[,\s]/u;

/** TUI status-badge rows ("▣  Build · <model>") — badge glyphs never gutter
 * message content — reasoning-duration rows ("+ Thought: 304ms"), and
 * tool-status morphs ("~ Preparing write...", "← Write file.txt"). */
const STATUS_BADGE = /^\s*[▣▢◈◉]\s/u;
const DURATION_ROW = /^\s*\+?\s*\p{L}[\p{L} ]*:\s*\d+(?:\.\d+)?\s*m?s\s*$/u;
const TOOL_STATUS_GLYPH = /^\s*[~←→↺✔✘✗]\s/u;

/** Thinking-verb spinner rows ("✢ Pontificating… (16s · ↓ 788 tokens)",
 * "· Garnishing…"): a lone gerund ending in an ellipsis, optional counter —
 * and nothing else, so prose that merely starts with "Waiting…" survives.
 * The completed form ("✻ Brewed for 2s") is the same status family. */
const THINKING_SPINNER = /^\s*[✢✳✶✷✻✽·∗⁕*+]?\s*\p{L}+(?:-\p{L}+)*[…⋯]\s*(?:\([^()]*\))?\s*$/u;
const COMPLETED_VERB = /^\s*[✢✳✶✷✻✽·∗⁕*+]\s*\p{L}+(?:-\p{L}+)*\s+for\s+\d+(?:m\s?\d+)?s\s*$/u;

/** Line-numbered file dumps ("      1 hello streaming") — tool output. */
const NUMBERED_FILE_ROW = /^\s{4,}\d+\s\S/u;

/** Tool-run blocks are canonical tool cards' content, never message text, and
 * they morph/collapse in place: single-word action headers ("• Explored"),
 * diffstat summaries ("… (+1 -0)"), and tool child/output rows at the
 * TUI's tool column (message continuations sit shallower, code deeper). */
const TOOL_HEADER_WORD = /^\s*[•⏺●○]\s+\p{L}+\s*$/u;
const DIFFSTAT_SUFFIX = /\(\+\d+\s+-\d+\)\s*$/u;
const TOOL_CHILD_ROW = /^ {4}\S/;

export function isPreviewJunkRow(line: string): boolean {
  return (
    line.trim().length === 0 ||
    JUNK_ROW.test(line) ||
    RULE_ROW.test(line) ||
    SPINNER_ROW.test(line) ||
    QUEUE_ROW.test(line) ||
    PAREN_FOOTER.test(line) ||
    PAREN_FRAGMENT.test(line) ||
    TIP_BANNER.test(line) ||
    USAGE_BANNER.test(line) ||
    INTERRUPT_HINT.test(line) ||
    KEY_HINT_ROW.test(line) ||
    PROGRESS_DOTS.test(line) ||
    MULTI_COLUMN_ROW.test(line) ||
    (COMPOSER_ECHO.test(line) && !OPTION_ROW.test(line)) ||
    MODEL_CWD_FOOTER.test(line) ||
    DIFF_ROW.test(line) ||
    CLI_HELP_ROW.test(line) ||
    STATUS_BADGE.test(line) ||
    DURATION_ROW.test(line) ||
    TOOL_STATUS_GLYPH.test(line) ||
    THINKING_SPINNER.test(line) ||
    COMPLETED_VERB.test(line) ||
    NUMBERED_FILE_ROW.test(line) ||
    TOOL_HEADER_WORD.test(line) ||
    DIFFSTAT_SUFFIX.test(line) ||
    TOOL_CHILD_ROW.test(line)
  );
}

/** A framed run is an interactive QUESTION dialog (content, not tool junk)
 * when it contains a checkbox/radio option row ("1. [ ] Smooth"). */
const FRAME_ROW = /^\s*[│┃]/u;
const FRAMED_OPTION = /^\s*[│┃]?\s*\d+[.)]\s*(?:\[[xX✓ ]*\]|\([xX✓• ]*\))/u;
const KEY_GLYPH_HINT = /[⇆⏎]|↑↓/u;

function hasQuestionDialog(rows: readonly string[]): boolean {
  return rows.some((row) => FRAMED_OPTION.test(row));
}

/**
 * Cap the preview window. Streaming content shows its LAST rows; a framed
 * question dialog instead anchors at its first row — options without their
 * question are unreadable, so the question + leading options win the window.
 */
export function capPreviewRows(rows: readonly string[]): string[] {
  for (let i = 0; i < rows.length; i++) {
    if (!FRAME_ROW.test(rows[i])) continue;
    let end = i;
    let hasOption = false;
    while (end < rows.length && FRAME_ROW.test(rows[end])) {
      if (FRAMED_OPTION.test(rows[end])) hasOption = true;
      end++;
    }
    if (hasOption) return rows.slice(i, i + MAX_PREVIEW_ROWS);
    i = end - 1;
  }
  return rows.slice(-MAX_PREVIEW_ROWS);
}

/** Rows worth previewing as message content: junk frames dropped, except a
 * framed interactive-question dialog, which the user must see. */
export function displayablePreview(lines: readonly string[]): string[] {
  const out: string[] = [];
  let i = 0;
  while (i < lines.length) {
    if (FRAME_ROW.test(lines[i])) {
      let end = i;
      let hasOption = false;
      while (end < lines.length && FRAME_ROW.test(lines[end])) {
        if (FRAMED_OPTION.test(lines[end])) hasOption = true;
        end++;
      }
      if (hasOption) {
        for (let k = i; k < end; k++) {
          if (!KEY_GLYPH_HINT.test(lines[k])) out.push(lines[k]);
        }
      }
      i = end;
      continue;
    }
    if (!isPreviewJunkRow(lines[i])) out.push(lines[i]);
    i++;
  }
  return out;
}

/** Per-turn display window state for [`stabilizePreview`]. */
export interface PreviewStabilizerState {
  turn: number;
  retired: Set<string>;
  last: string[];
  lastKey: string;
  lastResult: string[];
}

export function createPreviewStabilizerState(): PreviewStabilizerState {
  return { turn: -1, retired: new Set(), last: [], lastKey: "", lastResult: [] };
}

/**
 * The ONE smooth-transition definition, shared by the display stabilizer and
 * the offline jank classifier: cut rows from the top, append at the bottom,
 * and/or extend the final row (raw or normalized prefix — markdown
 * finalization may restyle the completing tail row in place).
 */
export function isSmoothPreviewTransition(
  prev: readonly string[],
  next: readonly string[],
): boolean {
  if (prev.length === 0 || next.length === 0) return true;
  // A question dialog opening or closing is a deliberate modal event the
  // user must see immediately — never churn to smooth over or hold back.
  if (hasQuestionDialog(prev) !== hasQuestionDialog(next)) return true;
  for (let drop = 0; drop < prev.length; drop++) {
    const kept = prev.slice(drop);
    if (kept.length > next.length) continue;
    let matches = true;
    for (let i = 0; i < kept.length; i++) {
      const isLastKept = i === kept.length - 1;
      if (next[i] === kept[i]) continue;
      if (isLastKept && extendsRow(kept[i], next[i])) continue;
      matches = false;
      break;
    }
    if (matches) return true;
  }
  return false;
}

function extendsRow(shown: string, next: string): boolean {
  return next.startsWith(shown) || normalizeStreamLine(next).startsWith(normalizeStreamLine(shown));
}

/**
 * Smooth-or-hold display window. Retired rows (cut from the top) never
 * re-enter; re-emitted earlier content re-anchors so shown rows keep their
 * position; any transition that still is not smooth HOLDS the current display
 * until the stream stabilizes. Empty input dies only when coverage emptied it
 * (`coverageEmptied`); a transient all-chrome frame holds instead of
 * blinking. A hold never outlives coverage: held rows the canon (`blob`)
 * already covers are cut so landed content dies even while holding.
 * Idempotent per (turn, rows, blob) so repeated renders are pure.
 */
export function stabilizePreview(
  state: PreviewStabilizerState,
  turnSeq: number,
  rows: readonly string[],
  coverageEmptied = true,
  blob = "",
): string[] {
  const key = `${turnSeq}\n${coverageEmptied ? "c" : "t"}\n${blob.length}\n${rows.join("\n")}`;
  if (key === state.lastKey) return state.lastResult;
  if (turnSeq !== state.turn) {
    state.turn = turnSeq;
    state.retired.clear();
    state.last = [];
  }
  // A question dialog is modal: it always displays whole, never trimmed by
  // earlier retirements (its rows may have scrolled out pre-anchor).
  let start = 0;
  if (!hasQuestionDialog(rows)) {
    while (start < rows.length && state.retired.has(rows[start])) start++;
  }
  let next = rows.slice(start);
  if (next.length === 0) {
    next = coverageEmptied ? [] : uncoveredPreviewTail(state.last, blob);
    state.last = next;
  } else if (state.last.length === 0) {
    state.last = next;
  } else {
    const shift = anchorShift(state.last, next);
    if (shift !== null && shift > 0) {
      for (let i = 0; i < shift && i < next.length; i++) state.retired.add(next[i]);
      next = next.slice(shift);
    }
    if (next.length > 0 && isSmoothPreviewTransition(state.last, next)) {
      const cut = state.last.indexOf(next[0]);
      if (cut > 0) {
        for (let i = 0; i < cut; i++) state.retired.add(state.last[i]);
      }
      state.last = next;
    } else {
      next = uncoveredPreviewTail(state.last, blob);
      state.last = next;
    }
  }
  state.lastKey = key;
  state.lastResult = next;
  return next;
}

const MIN_ANCHOR_CHARS = 8;

/** How far DOWN the previously shown rows moved in `next` (0 = in place,
 * null = no shown row survived — e.g. every row rewrapped). */
function anchorShift(prev: readonly string[], next: readonly string[]): number | null {
  let weak: number | null = null;
  for (let i = 0; i < prev.length; i++) {
    const j = next.indexOf(prev[i]);
    if (j < 0) continue;
    if (prev[i].trim().length >= MIN_ANCHOR_CHARS) return Math.max(0, j - i);
    if (weak === null) weak = Math.max(0, j - i);
  }
  return weak;
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
