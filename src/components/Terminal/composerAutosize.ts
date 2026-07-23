export const COMPOSER_MIN_HEIGHT = 20;
export const COMPOSER_HEIGHT_EPSILON = 2;
export const COMPOSER_MAX_HEIGHT = 230;

export function composerHeight(el: HTMLTextAreaElement): number {
  const measured = el.scrollHeight;
  if (measured <= COMPOSER_MIN_HEIGHT + COMPOSER_HEIGHT_EPSILON) {
    return COMPOSER_MIN_HEIGHT;
  }
  return Math.min(Math.max(measured, COMPOSER_MIN_HEIGHT), COMPOSER_MAX_HEIGHT);
}

/**
 * Autosize the composer textarea to fit its content and return the applied
 * height. Measuring requires collapsing the textarea to its minimum so
 * `scrollHeight` reports the content height — but the composer shell is a flex
 * sibling of the chat scroll viewport, so an uncompensated collapse momentarily
 * grows the viewport. A bottom-pinned transcript then gets its `scrollTop`
 * clamped off the bottom by the browser (nothing re-pins a net-zero resize),
 * producing per-keystroke scroll jitter. Freezing the shell's height across the
 * measurement keeps the viewport size constant, so the pinned position holds.
 */
export function applyComposerAutosize(
  textarea: HTMLTextAreaElement,
  shell: HTMLElement | null,
): number {
  const frozenHeight = shell?.offsetHeight ?? 0;
  const freeze = shell != null && frozenHeight > 0;
  if (freeze) shell.style.minHeight = `${frozenHeight}px`;
  try {
    textarea.style.height = `${COMPOSER_MIN_HEIGHT}px`;
    const next = composerHeight(textarea);
    textarea.style.height = `${next}px`;
    return next;
  } finally {
    if (freeze) shell.style.minHeight = "";
  }
}
