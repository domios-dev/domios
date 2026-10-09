export type ComposerMode = "floating" | "docked";
export interface ComposerLayout { mode: ComposerMode; editorPx: number; scrolls: boolean }
export const COMPOSER_FLOAT_MIN_WIDTH = 480;

export function resolveComposerLayout(input: {
  contentPx: number; minPx: number; editorMaxPx: number; panelWidthPx: number;
  panelHeightPx: number; keyboardOpen: boolean;
}): ComposerLayout {
  const max = Math.max(input.minPx, Math.min(input.editorMaxPx, input.panelHeightPx * 0.4));
  const editorPx = Math.max(input.minPx, Math.min(input.contentPx, max));
  const docked = input.keyboardOpen || input.panelWidthPx < COMPOSER_FLOAT_MIN_WIDTH;
  return { mode: docked ? "docked" : "floating", editorPx, scrolls: input.contentPx > max };
}
