// Regression for per-keystroke chat scroll jitter. The composer autosize must
// measure the textarea without letting the sibling chat viewport grow, or the

import { applyComposerAutosize, COMPOSER_MIN_HEIGHT } from "./composerAutosize";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

/**
 * Faithful layout model of the composer/viewport flex column.
 *
 * - The scroll viewport and the composer shell split a fixed column budget.
 * - The shell's rendered height is max(min-height, textarea box + chrome), so
 *   collapsing the textarea to measure it shrinks the shell unless frozen.
 * - Reading the textarea's scrollHeight forces a reflow: the viewport's
 *   clientHeight recomputes and the browser clamps scrollTop to the new bottom
 *   (clamping only ever reduces scrollTop — it is never restored on regrow).
 */
function makeFlexColumn(opts: { contentLines: number }) {
  const CHROME = 20; // shell padding/border + composer chrome around the textarea
  const COLUMN = 1000; // total height shared by viewport + shell
  const TRANSCRIPT = 1600; // scrollHeight of a long transcript
  const contentHeight = opts.contentLines * 20;

  const textareaStyle: { height: string } = { height: `${contentHeight}px` };
  const shellStyle: { minHeight: string } = { minHeight: "" };
  const viewport = { scrollTop: 0 };

  const shellHeight = () =>
    Math.max(parseInt(shellStyle.minHeight, 10) || 0, (parseInt(textareaStyle.height, 10) || 0) + CHROME);

  const relayout = () => {
    const clientHeight = COLUMN - shellHeight();
    const maxScrollTop = Math.max(0, TRANSCRIPT - clientHeight);
    if (viewport.scrollTop > maxScrollTop) viewport.scrollTop = maxScrollTop;
  };

  const textarea = {
    get style() { return textareaStyle; },
    // Content overflows the collapsed box, so scrollHeight reports content height.
    get scrollHeight() { return contentHeight; },
  } as unknown as HTMLTextAreaElement;

  // A height write is the forced-reflow point that reclamps the viewport.
  Object.defineProperty(textareaStyle, "height", {
    configurable: true,
    get() { return this._h ?? `${contentHeight}px`; },
    set(v: string) { this._h = v; relayout(); },
  });

  const shell = {
    get style() { return shellStyle; },
    get offsetHeight() { return shellHeight(); },
  } as unknown as HTMLElement;

  // Pin to bottom for the settled content height.
  viewport.scrollTop = TRANSCRIPT - (COLUMN - shellHeight());
  return { textarea, shell, viewport, pinnedBottom: viewport.scrollTop };
}

// --- 1. the unfrozen measurement drifts a pinned viewport off the bottom (the bug) ---
{
  const col = makeFlexColumn({ contentLines: 3 });
  const before = col.viewport.scrollTop;
  // Reproduce the old inline autosize: collapse to measure, restore — no freeze.
  col.textarea.style.height = `${COMPOSER_MIN_HEIGHT}px`;
  col.textarea.style.height = `${col.textarea.scrollHeight}px`;
  assert(
    col.viewport.scrollTop < before,
    `sanity: an unfrozen collapse must drift the pinned viewport off bottom (before=${before} after=${col.viewport.scrollTop})`,
  );
  console.log(`PASS unfrozen autosize drifts pinned scrollTop ${before} -> ${col.viewport.scrollTop} (the jitter)`);
}

// --- 2. applyComposerAutosize freezes the shell so the pinned viewport never moves ---
{
  const col = makeFlexColumn({ contentLines: 3 });
  const before = col.viewport.scrollTop;
  const next = applyComposerAutosize(col.textarea, col.shell);
  assert(next === 60, `autosize returns the measured content height (got ${next})`);
  assert(
    col.viewport.scrollTop === before,
    `pinned viewport stays glued to the bottom across a keystroke measurement (before=${before} after=${col.viewport.scrollTop})`,
  );
  assert(col.shell.style.minHeight === "", "the freeze is released after measuring");
  console.log("PASS frozen autosize holds the pinned viewport across measurement");
}

// --- 3. repeated keystrokes never accumulate drift ---
{
  const col = makeFlexColumn({ contentLines: 4 });
  const bottom = col.viewport.scrollTop;
  for (let i = 0; i < 10; i += 1) applyComposerAutosize(col.textarea, col.shell);
  assert(
    col.viewport.scrollTop === bottom,
    `ten keystrokes leave the pinned viewport exactly at bottom (expected ${bottom}, got ${col.viewport.scrollTop})`,
  );
  console.log("PASS repeated measurements never oscillate the viewport");
}

// --- 4. a single-line composer (no chrome growth) is unaffected and still measures min ---
{
  const col = makeFlexColumn({ contentLines: 1 });
  const before = col.viewport.scrollTop;
  const next = applyComposerAutosize(col.textarea, col.shell);
  assert(next === COMPOSER_MIN_HEIGHT, `single-line composer collapses to min height (got ${next})`);
  assert(col.viewport.scrollTop === before, "single-line measurement does not move the viewport");
  console.log("PASS single-line composer measurement is stable");
}

// --- 5. missing shell degrades safely (still measures, just cannot freeze) ---
{
  const col = makeFlexColumn({ contentLines: 3 });
  const next = applyComposerAutosize(col.textarea, null);
  assert(next === 60, `autosize still returns a height without a shell ref (got ${next})`);
  console.log("PASS autosize tolerates a missing shell ref");
}

console.log("composer autosize jitter checks passed");
