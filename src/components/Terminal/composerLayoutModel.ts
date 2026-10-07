export type ComposerMode = "floating" | "docked";
export interface ComposerLayout { mode: ComposerMode; editorPx: number; scrolls: boolean }
export const COMPOSER_DOCK_HYSTERESIS = 40;
export const COMPOSER_FLOAT_MIN_WIDTH = 480;

export function resolveComposerLayout(input: {
  contentPx: number; minPx: number; editorMaxPx: number; panelWidthPx: number;
  panelHeightPx: number; keyboardOpen: boolean; prev?: ComposerMode;
}): ComposerLayout {
  const max = Math.max(input.minPx, Math.min(input.editorMaxPx, input.panelHeightPx * 0.4));
  const editorPx = Math.max(input.minPx, Math.min(input.contentPx, max));
  const forced = input.keyboardOpen || input.panelWidthPx < COMPOSER_FLOAT_MIN_WIDTH;
  const cleared = input.contentPx <= input.minPx;
  const held = input.prev === "docked" && input.contentPx >= max - COMPOSER_DOCK_HYSTERESIS;
  return { mode: forced || !cleared && (input.contentPx >= max || held) ? "docked" : "floating",
    editorPx, scrolls: input.contentPx > max };
}
