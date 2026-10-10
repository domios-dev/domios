export const COMPOSER_MIN_HEIGHT = 20;
export const COMPOSER_HEIGHT_EPSILON = 2;
export const COMPOSER_MAX_HEIGHT = 230;

export function composerHeight(el: HTMLTextAreaElement, maxPx = COMPOSER_MAX_HEIGHT): number {
  return clampComposerHeight(el.scrollHeight, maxPx);
}

function clampComposerHeight(measured: number, maxPx: number): number {
  if (measured <= COMPOSER_MIN_HEIGHT + COMPOSER_HEIGHT_EPSILON) return COMPOSER_MIN_HEIGHT;
  return Math.min(Math.max(measured, COMPOSER_MIN_HEIGHT), Math.max(maxPx, COMPOSER_MIN_HEIGHT));
}

export function measureComposerAutosize(textarea: HTMLTextAreaElement, shell: HTMLElement | null, maxPx = COMPOSER_MAX_HEIGHT) {
  const frozenHeight = shell?.offsetHeight ?? 0;
  const freeze = shell != null && frozenHeight > 0;
  const previousMinHeight = shell?.style.minHeight ?? "";
  if (freeze) shell.style.minHeight = `${frozenHeight}px`;
  try {
    textarea.style.height = `${COMPOSER_MIN_HEIGHT}px`;
    const contentPx = textarea.scrollHeight;
    const editorPx = clampComposerHeight(contentPx, maxPx);
    textarea.style.height = `${editorPx}px`;
    return { contentPx, editorPx };
  } finally {
    if (freeze) shell.style.minHeight = previousMinHeight;
  }
}

export function applyComposerAutosize(textarea: HTMLTextAreaElement, shell: HTMLElement | null): number {
  return measureComposerAutosize(textarea, shell).editorPx;
}
