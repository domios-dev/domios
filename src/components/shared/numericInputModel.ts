export interface NumericParseOptions {
  integer?: boolean;
  min?: number;
  max?: number;
}

export interface NumericStepOptions extends NumericParseOptions {
  step?: number;
}

export type PendingNumericEmission = { kind: "value"; value: number } | { kind: "empty" };

export function matchesPendingEmission(
  emission: PendingNumericEmission,
  external: number | null | undefined,
): boolean {
  if (emission.kind === "empty") return external == null;
  return external != null && Object.is(emission.value, external);
}

export function lastCommittedNumeric(
  pending: readonly PendingNumericEmission[],
  external: number | null | undefined,
): PendingNumericEmission {
  if (pending.length > 0) return pending[pending.length - 1];
  return external == null ? { kind: "empty" } : { kind: "value", value: external };
}

export function formatPendingEmission(emission: PendingNumericEmission): string {
  return emission.kind === "empty" ? "" : formatNumericInput(emission.value);
}

export function reconcileExternalNumericValue(
  pending: readonly PendingNumericEmission[],
  external: number | null | undefined,
  focused: boolean,
): { pending: PendingNumericEmission[]; nextDraft: string | null } {
  if (pending.length > 0) {
    const matchIndex = pending.findIndex((item) => matchesPendingEmission(item, external));
    if (matchIndex >= 0) {
      const remaining = pending.slice(matchIndex + 1);
      if (focused || remaining.length > 0) {
        return { pending: remaining, nextDraft: null };
      }
      return { pending: [], nextDraft: formatNumericInput(external) };
    }
    if (focused) {
      return { pending: [...pending], nextDraft: null };
    }
    return { pending: [], nextDraft: formatNumericInput(external) };
  }
  if (focused) {
    return { pending: [], nextDraft: null };
  }
  return { pending: [], nextDraft: formatNumericInput(external) };
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
