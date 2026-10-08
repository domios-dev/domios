export const LOG_MIN_HEIGHT = 120;
export const GRAPH_MIN_HEIGHT = 180;
export const LOG_DEFAULT_HEIGHT = 260;
export const LOG_HEIGHT_KEY = "codeterm.blueprint.runLogHeight";
const KEY_STEP = 16;
const PAGE_STEP = 96;

export function logHeightBounds(available: number) {
  return { min: LOG_MIN_HEIGHT, max: Math.max(LOG_MIN_HEIGHT, available - GRAPH_MIN_HEIGHT) };
}

export function clampLogHeight(height: number, available: number): number {
  const { min, max } = logHeightBounds(available);
  return Math.round(Math.min(max, Math.max(min, height)));
}

/** Dragging the divider up grows the log under it. */
export function dragLogHeight(startHeight: number, startY: number, currentY: number, available: number): number {
  return clampLogHeight(startHeight + startY - currentY, available);
}

/** The separator's keyboard contract: arrows nudge, Page keys jump, Home and End reach the bounds. */
export function keyLogHeight(height: number, code: string, available: number): number | null {
  const { min, max } = logHeightBounds(available);
  const next = code === "ArrowUp" ? height + KEY_STEP
    : code === "ArrowDown" ? height - KEY_STEP
      : code === "PageUp" ? height + PAGE_STEP
        : code === "PageDown" ? height - PAGE_STEP
          : code === "Home" ? min
            : code === "End" ? max
              : null;
  return next === null ? null : clampLogHeight(next, available);
}

export function readLogHeight(storage: Pick<Storage, "getItem"> | null): number {
  try {
    const stored = Number(storage?.getItem(LOG_HEIGHT_KEY));
    return Number.isFinite(stored) && stored >= LOG_MIN_HEIGHT ? stored : LOG_DEFAULT_HEIGHT;
  } catch {
    return LOG_DEFAULT_HEIGHT;
  }
}

export function writeLogHeight(storage: Pick<Storage, "setItem"> | null, height: number) {
  try {
    storage?.setItem(LOG_HEIGHT_KEY, String(Math.round(height)));
  } catch {
    // Storage can be unavailable; the divider still works for this session.
  }
}
