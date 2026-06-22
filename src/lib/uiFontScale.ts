export const UI_FONT_SCALE_MIN = -1;
export const UI_FONT_SCALE_MAX = 2;
const COMFORTABLE_FONT_SIZE = 14;

export function fontSizeToUiFontScale(fontSize: unknown): number {
  const parsed = typeof fontSize === "number" ? fontSize : Number(fontSize);
  if (!Number.isFinite(parsed)) return 1;
  return Math.max(
    UI_FONT_SCALE_MIN,
    Math.min(UI_FONT_SCALE_MAX, Math.round(parsed) - COMFORTABLE_FONT_SIZE + 1),
  );
}

export function pickUiFontScaleValue<T>(scale: unknown, values: readonly [T, T, T, T]): T {
  const parsed = typeof scale === "number" ? scale : Number(scale);
  const normalized = Number.isFinite(parsed)
    ? Math.max(UI_FONT_SCALE_MIN, Math.min(UI_FONT_SCALE_MAX, Math.round(parsed)))
    : fontSizeToUiFontScale(COMFORTABLE_FONT_SIZE);
  return values[normalized - UI_FONT_SCALE_MIN];
}
