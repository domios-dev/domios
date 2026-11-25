export interface NumericParseOptions {
  integer?: boolean;
  min?: number;
  max?: number;
}

export interface NumericStepOptions extends NumericParseOptions {
  step?: number;
}

const INTEGER_PATTERN = /^[+-]?\d+$/;
const DECIMAL_PATTERN = /^[+-]?(?:\d+(?:\.\d+)?|\.\d+)$/;

export function formatNumericInput(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return "";
  return Object.is(value, -0) ? "0" : String(value);
}

export function parseNumericInput(raw: string, options: NumericParseOptions = {}): number | null {
  const trimmed = raw.trim();
  if (trimmed === "") return null;
  const pattern = options.integer ? INTEGER_PATTERN : DECIMAL_PATTERN;
  if (!pattern.test(trimmed)) return null;
  const parsed = Number(trimmed);
  if (!Number.isFinite(parsed)) return null;
  if (options.integer && !Number.isInteger(parsed)) return null;
  if (options.min !== undefined && parsed < options.min) return null;
  if (options.max !== undefined && parsed > options.max) return null;
  return Object.is(parsed, -0) ? 0 : parsed;
}

function decimalPlaces(value: number): number {
  const text = String(value).toLowerCase();
  const expIdx = text.indexOf("e-");
  const exp = expIdx >= 0 ? Number(text.slice(expIdx + 2)) : 0;
  const base = expIdx >= 0 ? text.slice(0, expIdx) : text;
  const dot = base.indexOf(".");
  const frac = dot >= 0 ? base.length - dot - 1 : 0;
  return frac + exp;
}

export function stepNumericValue(
  current: number | null | undefined,
  direction: 1 | -1,
  options: NumericStepOptions = {},
): number {
  const step = options.step && Number.isFinite(options.step) && options.step > 0 ? options.step : 1;
  const base = current != null && Number.isFinite(current) ? current : (options.min ?? 0);
  const nextRaw = base + direction * step;
  const precision = options.integer ? 0 : Math.min(12, Math.max(decimalPlaces(base), decimalPlaces(step)));
  let next = precision > 0 ? Number(nextRaw.toFixed(precision)) : Math.round(nextRaw);
  if (options.min !== undefined && next < options.min) next = options.min;
  if (options.max !== undefined && next > options.max) next = options.max;
  return Object.is(next, -0) ? 0 : next;
}
