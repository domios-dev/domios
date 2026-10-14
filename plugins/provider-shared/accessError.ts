function object(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;
}

export type AccessErrorClass = "authentication" | "auth_expiry" | "usage_limit" | "rate_limit" | "server_overloaded" | "other";

const authExpiry = ["token_expired", "refresh_token_expired", "auth_expired", "auth_expiry"];
const authentication = [
  "401", "403", "authentication_error", "auth", "unauthorized", "permission_error", "permission_denied",
  "forbidden", "subscription_disabled", "organization_disabled", "oauth_refresh_failed", "token_revoked",
  "refresh_token_reused", "refresh_token_invalidated", "logged_out", "signed_out", "unauthenticated", "authError",
];
const usage = ["usageLimitExceeded", "usage_limit", "sessionBudgetExceeded"];
const rate = ["429", "rate_limit", "rate_limit_error", "rateLimitExceeded"];
const server = ["overloaded", "overloaded_error", "serverOverloaded", "server_overloaded", "internalServerError", "internal_server_error"];

function statusCode(value: unknown): number | null {
  if (typeof value === "number" && Number.isInteger(value)) return value;
  if (typeof value === "string" && /^\d+$/.test(value)) return Number(value);
  return null;
}

export function classifyAccessError(error: unknown, record: Record<string, unknown>): AccessErrorClass {
  const detail = object(error);
  const info = detail?.codexErrorInfo;
  const infoObject = object(info);
  const fields: unknown[] = [
    typeof error === "string" ? error : null, detail?.type, detail?.code, detail?.status, detail?.statusCode,
    detail?.status_code, detail?.httpStatusCode, typeof info === "string" ? info : null,
    ...Object.keys(infoObject ?? {}), ...Object.values(infoObject ?? {}).map(value => object(value)?.httpStatusCode),
    record.status, record.statusCode, record.status_code, record.apiErrorStatus, record.apiErrorCode,
  ];
  const named = (codes: string[]) => fields.some(value => typeof value === "string" && codes.includes(value));
  const prefixed = (prefix: string) => fields.some(value => typeof value === "string" && value.startsWith(prefix));
  const status = (match: (code: number) => boolean) => fields.some(value => {
    const code = statusCode(value);
    return code != null && match(code);
  });
  if (named(authExpiry)) return "auth_expiry";
  if (named(["authError"]) || prefixed("oauth_") || prefixed("authentication_") || status(code => code === 401 || code === 403) || named(authentication)) return "authentication";
  if (named(usage)) return "usage_limit";
  if (status(code => code === 429) || named(rate)) return "rate_limit";
  if (status(code => code >= 500 && code <= 599) || named(server)) return "server_overloaded";
  return "other";
}

function readRetry(value: Record<string, unknown> | null): number | null {
  if (!value) return null;
  for (const key of ["retryAfter", "retry_after", "retryAfterSeconds", "retry_after_secs", "retryAfterSecs"]) {
    const field = value[key];
    if (typeof field === "number" && Number.isInteger(field) && field >= 0) return field;
    if (typeof field === "string" && /^\d+$/.test(field)) return Number(field);
  }
  return null;
}

export function retryAfterSecs(error: unknown, record: Record<string, unknown>): number | null {
  return readRetry(object(error)) ?? readRetry(record);
}

export function observationTurnId(record: Record<string, unknown>): string | null {
  for (const key of ["uuid", "turn_id", "turnId", "requestId"]) {
    const value = record[key];
    if (typeof value === "string" && value) return value;
  }
  const payload = object(record.payload);
  if (!payload) return null;
  for (const key of ["turn_id", "turnId", "uuid", "requestId"]) {
    const value = payload[key];
    if (typeof value === "string" && value) return value;
  }
  return null;
}

export function observationRetrying(error: unknown, record: Record<string, unknown>): boolean {
  return record.willRetry === true || object(error)?.willRetry === true;
}

export function observationExtras(error: unknown, record: Record<string, unknown>): {retry_after_secs?: number; turn_id?: string; retrying?: true} {
  const extras: {retry_after_secs?: number; turn_id?: string; retrying?: true} = {};
  const retry = retryAfterSecs(error, record);
  if (retry != null) extras.retry_after_secs = retry;
  const turn = observationTurnId(record);
  if (turn) extras.turn_id = turn;
  if (observationRetrying(error, record)) extras.retrying = true;
  return extras;
}
