import type { JsonValue, RpcError, RuntimeErrorSignal } from "@codeterm/plugin-sdk";

export const TEXT_RESET_SAFETY_MARGIN_MS = 5_000;
export const MAX_RETRY_WINDOW_MS = 31 * 24 * 60 * 60 * 1000;
const MAX_SAFE_MS = 9_007_199_254_740_991;
const MAX_DATA_DEPTH = 4;
const MAX_ARRAY_ITEMS = 32;
const MAX_JSON_STRING_BYTES = 16_384;
const MAX_TEXT_SCAN_CHARS = 8_192;

const AUTH_OR_CLIENT_HTTP = new Set([400, 401, 403, 404, 405, 422]);
const AUTH_TOKENS = new Set([
  "unauthenticated",
  "permission_denied",
  "permissiondenied",
  "authentication_error",
  "authenticationerror",
  "authentication_required",
  "authenticationrequired",
  "unauthorized",
  "forbidden",
  "auth_required",
  "authrequired",
  "auth_expired",
  "authexpired",
  "auth_expiry",
  "authexpiry",
  "token_expired",
  "tokenexpired",
]);
const QUOTA_TOKENS = new Set([
  "429",
  "resource_exhausted",
  "resourceexhausted",
  "rate_limit",
  "ratelimit",
  "rate_limited",
  "ratelimited",
  "rate_limit_exceeded",
  "ratelimitexceeded",
  "rate_limit_error",
  "ratelimiterror",
  "rate_limited_until",
  "ratelimiteduntil",
  "too_many_requests",
  "toomanyrequests",
  "quota",
  "quota_exceeded",
  "quotaexceeded",
  "quota_reached",
  "quotareached",
  "quota_exhausted",
  "quotaexhausted",
  "insufficient_quota",
  "insufficientquota",
  "usage_limit",
  "usagelimit",
  "usage_limit_exceeded",
  "usagelimitexceeded",
]);

const CLASSIFIER_KEYS = [
  "status",
  "code",
  "type",
  "reason",
  "errorCode",
  "error_code",
  "kind",
  "class",
  "name",
  "error",
] as const;
const HTTP_STATUS_KEYS = [
  "status",
  "statusCode",
  "status_code",
  "httpStatusCode",
  "http_status",
  "code",
] as const;
const EPOCH_MS_KEYS = [
  "resetsAtMs",
  "resetAtMs",
  "retryAtMs",
  "resets_at_ms",
  "reset_at_ms",
  "retry_at_ms",
] as const;
const TIMESTAMP_KEYS = [
  "resetsAt",
  "resetAt",
  "resetTime",
  "resets_at",
  "reset_at",
  "reset_time",
  "quotaResetTime",
  "quota_reset_time",
] as const;
const DELAY_MS_KEYS = [
  "retryAfterMs",
  "retry_after_ms",
  "quotaResetDelayMs",
  "quota_reset_delay_ms",
  "resetInMs",
  "reset_in_ms",
] as const;
const DELAY_SECONDS_KEYS = [
  "retryAfterSeconds",
  "retry_after_secs",
  "retryAfterSecs",
  "retry_after_seconds",
] as const;
const RETRY_AFTER_KEYS = [
  "retryAfter",
  "retry_after",
  "Retry-After",
  "retry-after",
] as const;
const RETRY_DELAY_KEYS = [
  "retryDelay",
  "retry_delay",
  "quotaResetDelay",
  "quota_reset_delay",
] as const;
const MESSAGE_KEYS = ["message", "detail", "errorMessage", "description"] as const;

const STRICT_ISO_8601 =
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:?\d{2})$/;
const HTTP_DATE =
  /^(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun),\s+\d{2}\s+(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s+\d{4}\s+\d{2}:\d{2}:\d{2}\s+GMT$/;
const EXPLICIT_QUOTA_TEXT =
  /\b(?:quota(?:\s+(?:reached|exceeded|exhausted|limit))?|rate[\s_-]*limit(?:ed|s|\s+exceeded|\s+reached)?|usage\s+limit(?:\s+(?:reached|exceeded))?|too\s+many\s+requests|resource[_\s-]*exhausted|capacity\s+limit(?:\s+(?:reached|exceeded))?|limit\s+reached)\b/i;
const TEXT_ISO_CUE =
  /\b(?:resets?|retry|try\s+again|available(?:\s+again)?)\s+(?:at|after)\s+(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:?\d{2}))(?=$|[\s.,;)\]])/i;
const TEXT_DURATION_CUE =
  /\b(?:resets?|retry|try\s+again|available(?:\s+again)?|wait)\s+(?:in|after)\s+([0-9]+(?:\.[0-9]+)?\s*(?:days?|d|hours?|hrs?|h|minutes?|mins?|m|seconds?|secs?|s)(?:(?:\s*,?\s*|\s+and\s+)[0-9]+(?:\.[0-9]+)?\s*(?:hours?|hrs?|h|minutes?|mins?|m|seconds?|secs?|s))*)(?=$|[.,;)\]]|\s+(?![0-9]+[a-z]))/i;

const UNIT_MS: Readonly<Record<string, { rank: number; ms: number }>> = {
  d: { rank: 4, ms: 86_400_000 },
  day: { rank: 4, ms: 86_400_000 },
  days: { rank: 4, ms: 86_400_000 },
  h: { rank: 3, ms: 3_600_000 },
  hr: { rank: 3, ms: 3_600_000 },
  hrs: { rank: 3, ms: 3_600_000 },
  hour: { rank: 3, ms: 3_600_000 },
  hours: { rank: 3, ms: 3_600_000 },
  m: { rank: 2, ms: 60_000 },
  min: { rank: 2, ms: 60_000 },
  mins: { rank: 2, ms: 60_000 },
  minute: { rank: 2, ms: 60_000 },
  minutes: { rank: 2, ms: 60_000 },
  s: { rank: 1, ms: 1_000 },
  sec: { rank: 1, ms: 1_000 },
  secs: { rank: 1, ms: 1_000 },
  second: { rank: 1, ms: 1_000 },
  seconds: { rank: 1, ms: 1_000 },
  ms: { rank: 0, ms: 1 },
};

function isObject(value: unknown): value is Record<string, JsonValue> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function ownValue(entry: Record<string, JsonValue>, key: string): JsonValue | undefined {
  return Object.prototype.hasOwnProperty.call(entry, key) ? entry[key] : undefined;
}

function normalizeToken(value: string): string {
  return value.trim().toLowerCase().replace(/[\s-]+/g, "_");
}

function parseFiniteNonNegNumber(value: unknown): number | null {
  if (typeof value === "number") {
    return Number.isFinite(value) && value >= 0 ? value : null;
  }
  if (typeof value === "string") {
    const trimmed = value.trim();
    if (!/^\d+(?:\.\d+)?$/.test(trimmed)) return null;
    const parsed = Number(trimmed);
    return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
  }
  return null;
}

function parsePositiveInt(value: unknown): number | null {
  if (typeof value === "number") {
    return Number.isSafeInteger(value) && value > 0 ? value : null;
  }
  if (typeof value === "string") {
    const trimmed = value.trim();
    if (!/^\d+$/.test(trimmed)) return null;
    const parsed = Number(trimmed);
    return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : null;
  }
  return null;
}

function validFutureTimestamp(ms: number, nowMs: number): number | null {
  if (!Number.isSafeInteger(ms) || ms <= nowMs || ms > MAX_SAFE_MS) return null;
  if (ms - nowMs > MAX_RETRY_WINDOW_MS) return null;
  return ms;
}

function parseEpochOrIsoMs(value: unknown, nowMs: number, allowHttpDate = false): number | null {
  if (typeof value === "number") {
    if (!Number.isFinite(value) || value <= 0) return null;
    const ms = value >= 1e12 ? Math.trunc(value) : value >= 1e9 ? Math.trunc(value * 1000) : NaN;
    return Number.isFinite(ms) ? validFutureTimestamp(ms, nowMs) : null;
  }
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  if (!trimmed || trimmed.length > 128) return null;
  if (/^\d+(?:\.\d+)?$/.test(trimmed)) {
    const numeric = Number(trimmed);
    if (!Number.isFinite(numeric) || numeric <= 0) return null;
    const ms = numeric >= 1e12 ? Math.trunc(numeric) : numeric >= 1e9 ? Math.trunc(numeric * 1000) : NaN;
    return Number.isFinite(ms) ? validFutureTimestamp(ms, nowMs) : null;
  }
  if (STRICT_ISO_8601.test(trimmed) || (allowHttpDate && HTTP_DATE.test(trimmed))) {
    const parsed = Date.parse(trimmed);
    return Number.isFinite(parsed) ? validFutureTimestamp(Math.trunc(parsed), nowMs) : null;
  }
  return null;
}

/**
 * Parses an exact positive compound duration such as `51m16s`, `1h2m3s`, `3076s`, `3076.5s`,
 * or `2 hours 15 minutes`. Rejects zero, negative, duplicate, out-of-order, or absurd durations.
 */
export function parseExactDurationMs(raw: string): number | null {
  const trimmed = raw.trim();
  if (!trimmed || trimmed.length > 128) return null;
  const tokenPattern = /([0-9]+(?:\.[0-9]+)?)\s*(days?|d|hours?|hrs?|h|minutes?|mins?|ms|m|seconds?|secs?|s)(?![A-Za-z])/gi;
  let totalMs = 0;
  let lastRank = 99;
  let cursor = 0;
  let count = 0;
  for (;;) {
    const match = tokenPattern.exec(trimmed);
    if (!match) break;
    const gap = trimmed.slice(cursor, match.index);
    if (cursor === 0) {
      if (gap.length !== 0) return null;
    } else if (!/^(?:(?:\s*,?\s*)|\s+and\s+)$/i.test(gap)) {
      return null;
    }
    const amount = Number(match[1]);
    const unitKey = match[2].toLowerCase();
    const spec = Object.prototype.hasOwnProperty.call(UNIT_MS, unitKey) ? UNIT_MS[unitKey] : undefined;
    if (!spec || !Number.isFinite(amount) || amount < 0 || spec.rank >= lastRank) return null;
    lastRank = spec.rank;
    totalMs += amount * spec.ms;
    if (!Number.isFinite(totalMs) || totalMs > MAX_RETRY_WINDOW_MS) return null;
    cursor = match.index + match[0].length;
    count++;
  }
  if (count === 0 || cursor !== trimmed.length) return null;
  const rounded = Math.ceil(totalMs);
  if (!Number.isSafeInteger(rounded) || rounded <= 0 || rounded > MAX_RETRY_WINDOW_MS) return null;
  return rounded;
}

function parseProtoDurationMs(value: unknown): number | null {
  if (typeof value === "string") return parseExactDurationMs(value);
  if (typeof value === "number") {
    if (!Number.isFinite(value) || value <= 0) return null;
    const ms = Math.ceil(value * 1000);
    return Number.isSafeInteger(ms) && ms > 0 && ms <= MAX_RETRY_WINDOW_MS ? ms : null;
  }
  if (!isObject(value)) return null;
  const seconds = parseFiniteNonNegNumber(ownValue(value, "seconds"));
  const nanosRaw = ownValue(value, "nanos");
  const nanos = nanosRaw === undefined ? 0 : parseFiniteNonNegNumber(nanosRaw);
  if (seconds === null || nanos === null || nanos >= 1_000_000_000) return null;
  const ms = Math.ceil(seconds * 1000 + nanos / 1_000_000);
  return Number.isSafeInteger(ms) && ms > 0 && ms <= MAX_RETRY_WINDOW_MS ? ms : null;
}

function collectObjects(root: JsonValue | undefined): { objects: Record<string, JsonValue>[]; stringData?: string } {
  const objects: Record<string, JsonValue>[] = [];
  let stringData: string | undefined;
  let initial: JsonValue | undefined = root;
  if (typeof root === "string") {
    const trimmed = root.trim();
    if (trimmed.startsWith("{") && trimmed.length <= MAX_JSON_STRING_BYTES) {
      try {
        const parsed = JSON.parse(trimmed) as JsonValue;
        if (isObject(parsed) || Array.isArray(parsed)) initial = parsed;
        else stringData = root;
      } catch {
        stringData = root;
      }
    } else {
      stringData = root;
    }
  }
  const visit = (value: unknown, depth: number): void => {
    if (depth > MAX_DATA_DEPTH || value === null || typeof value !== "object") return;
    if (Array.isArray(value)) {
      const limit = Math.min(value.length, MAX_ARRAY_ITEMS);
      for (let i = 0; i < limit; i++) visit(value[i], depth + 1);
      return;
    }
    const obj = value as Record<string, JsonValue>;
    objects.push(obj);
    const keys = Object.keys(obj);
    for (let i = 0; i < keys.length; i++) {
      const child = obj[keys[i]];
      if (child !== null && typeof child === "object") visit(child, depth + 1);
    }
  };
  visit(initial, 0);
  return { objects, stringData };
}

function isDisqualifiedByAuthOrClientError(objects: readonly Record<string, JsonValue>[]): boolean {
  for (let i = 0; i < objects.length; i++) {
    const entry = objects[i];
    for (let k = 0; k < HTTP_STATUS_KEYS.length; k++) {
      const status = parsePositiveInt(ownValue(entry, HTTP_STATUS_KEYS[k]));
      if (status !== null && AUTH_OR_CLIENT_HTTP.has(status)) return true;
    }
    for (let k = 0; k < CLASSIFIER_KEYS.length; k++) {
      const raw = ownValue(entry, CLASSIFIER_KEYS[k]);
      if (typeof raw === "string" && AUTH_TOKENS.has(normalizeToken(raw))) return true;
    }
  }
  return false;
}

function hasStructuredQuotaIndicator(code: number, objects: readonly Record<string, JsonValue>[]): boolean {
  if (code === 429) return true;
  for (let i = 0; i < objects.length; i++) {
    const entry = objects[i];
    for (let k = 0; k < HTTP_STATUS_KEYS.length; k++) {
      if (parsePositiveInt(ownValue(entry, HTTP_STATUS_KEYS[k])) === 429) return true;
    }
    for (let k = 0; k < CLASSIFIER_KEYS.length; k++) {
      const raw = ownValue(entry, CLASSIFIER_KEYS[k]);
      if (typeof raw === "string" && QUOTA_TOKENS.has(normalizeToken(raw))) return true;
    }
    const typeTag = ownValue(entry, "@type");
    if (typeof typeTag === "string" && typeTag.indexOf("RetryInfo") !== -1) return true;
  }
  return false;
}

function extractStructuredResetMs(objects: readonly Record<string, JsonValue>[], nowMs: number): number | null {
  for (let i = 0; i < objects.length; i++) {
    const entry = objects[i];
    for (let k = 0; k < EPOCH_MS_KEYS.length; k++) {
      const parsed = parsePositiveInt(ownValue(entry, EPOCH_MS_KEYS[k]));
      if (parsed !== null) {
        const valid = validFutureTimestamp(parsed, nowMs);
        if (valid !== null) return valid;
      }
    }
  }
  for (let i = 0; i < objects.length; i++) {
    const entry = objects[i];
    for (let k = 0; k < TIMESTAMP_KEYS.length; k++) {
      const valid = parseEpochOrIsoMs(ownValue(entry, TIMESTAMP_KEYS[k]), nowMs);
      if (valid !== null) return valid;
    }
  }
  for (let i = 0; i < objects.length; i++) {
    const entry = objects[i];
    for (let k = 0; k < DELAY_MS_KEYS.length; k++) {
      const raw = parseFiniteNonNegNumber(ownValue(entry, DELAY_MS_KEYS[k]));
      if (raw !== null && raw > 0) {
        const delayMs = Math.ceil(raw);
        if (delayMs > 0 && delayMs <= MAX_RETRY_WINDOW_MS) {
          const valid = validFutureTimestamp(nowMs + delayMs, nowMs);
          if (valid !== null) return valid;
        }
      }
    }
  }
  for (let i = 0; i < objects.length; i++) {
    const entry = objects[i];
    for (let k = 0; k < DELAY_SECONDS_KEYS.length; k++) {
      const secs = parseFiniteNonNegNumber(ownValue(entry, DELAY_SECONDS_KEYS[k]));
      if (secs !== null && secs > 0) {
        const delayMs = Math.ceil(secs * 1000);
        if (delayMs > 0 && delayMs <= MAX_RETRY_WINDOW_MS) {
          const valid = validFutureTimestamp(nowMs + delayMs, nowMs);
          if (valid !== null) return valid;
        }
      }
    }
    for (let k = 0; k < RETRY_AFTER_KEYS.length; k++) {
      const raw = ownValue(entry, RETRY_AFTER_KEYS[k]);
      if (raw === undefined || raw === null) continue;
      const secs = parseFiniteNonNegNumber(raw);
      if (secs !== null && secs > 0) {
        const delayMs = Math.ceil(secs * 1000);
        if (delayMs > 0 && delayMs <= MAX_RETRY_WINDOW_MS) {
          const valid = validFutureTimestamp(nowMs + delayMs, nowMs);
          if (valid !== null) return valid;
        }
      } else if (typeof raw === "string") {
        const durationMs = parseExactDurationMs(raw);
        if (durationMs !== null) {
          const valid = validFutureTimestamp(nowMs + durationMs, nowMs);
          if (valid !== null) return valid;
        }
        const timestamp = parseEpochOrIsoMs(raw, nowMs, true);
        if (timestamp !== null) return timestamp;
      }
    }
    for (let k = 0; k < RETRY_DELAY_KEYS.length; k++) {
      const raw = ownValue(entry, RETRY_DELAY_KEYS[k]);
      if (raw === undefined || raw === null) continue;
      const delayMs = parseProtoDurationMs(raw);
      if (delayMs !== null) {
        const valid = validFutureTimestamp(nowMs + delayMs, nowMs);
        if (valid !== null) return valid;
      }
    }
  }
  return null;
}

function extractTextResetMs(
  candidates: readonly string[],
  structuredQuota: boolean,
  nowMs: number,
): number | null {
  for (let i = 0; i < candidates.length; i++) {
    const text = candidates[i].slice(0, MAX_TEXT_SCAN_CHARS);
    if (!structuredQuota && !EXPLICIT_QUOTA_TEXT.test(text)) continue;
    const isoMatch = TEXT_ISO_CUE.exec(text);
    if (isoMatch) {
      const parsed = Date.parse(isoMatch[1]);
      if (Number.isFinite(parsed)) {
        const valid = validFutureTimestamp(Math.trunc(parsed), nowMs);
        if (valid !== null) return valid;
      }
    }
    const durationMatch = TEXT_DURATION_CUE.exec(text);
    if (durationMatch) {
      const durationMs = parseExactDurationMs(durationMatch[1]);
      if (durationMs !== null) {
        const valid = validFutureTimestamp(nowMs + durationMs + TEXT_RESET_SAFETY_MARGIN_MS, nowMs);
        if (valid !== null) return valid;
      }
    }
  }
  return null;
}

/**
 * Classifies an ACP `session/prompt` JSON-RPC error as a deterministic `rate_limited_until`
 * signal only when structured error metadata or an explicit quota/rate-limit refusal carries
 * an exact reset timestamp or duration. Never guesses reset times.
 */
export function classifyPromptQuotaError(
  error: RpcError,
  nowMs: number = Date.now(),
): RuntimeErrorSignal | null {
  const now = Math.trunc(nowMs);
  if (!Number.isSafeInteger(now) || now < 0) return null;
  if (error.code === -32000) return null;
  const { objects, stringData } = collectObjects(error.data);
  if (isDisqualifiedByAuthOrClientError(objects)) return null;

  const structuredResetMs = extractStructuredResetMs(objects, now);
  if (structuredResetMs !== null) {
    return {
      retryable: false,
      retryableKind: "rate_limited_until",
      retryAtMs: structuredResetMs,
    };
  }

  const structuredQuota = hasStructuredQuotaIndicator(error.code, objects);
  const candidates: string[] = [];
  if (typeof error.message === "string" && error.message) candidates.push(error.message);
  if (stringData) candidates.push(stringData);
  for (let i = 0; i < objects.length; i++) {
    for (let k = 0; k < MESSAGE_KEYS.length; k++) {
      const text = ownValue(objects[i], MESSAGE_KEYS[k]);
      if (typeof text === "string" && text) candidates.push(text);
    }
  }
  const textResetMs = extractTextResetMs(candidates, structuredQuota, now);
  if (textResetMs !== null) {
    return {
      retryable: false,
      retryableKind: "rate_limited_until",
      retryAtMs: textResetMs,
    };
  }
  return null;
}
