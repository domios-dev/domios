const UI_FONT_SCALE_MIN = -1;
const UI_FONT_SCALE_MAX = 2;

export function pickUiFontScaleValue<T>(scale: unknown, values: readonly [T, T, T, T]): T {
  const parsed = typeof scale === "number" ? scale : Number(scale);
  const normalized = Number.isFinite(parsed)
    ? Math.max(UI_FONT_SCALE_MIN, Math.min(UI_FONT_SCALE_MAX, Math.round(parsed)))
    : 1;
  return values[normalized - UI_FONT_SCALE_MIN];
}
