function object(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;
}

export function classifyAccessError(error: unknown, record: Record<string, unknown>): "authentication" | "rate_limit" | "other" {
  const detail = object(error);
  const info = object(detail?.codexErrorInfo);
  const fields = [typeof error === "string" ? error : null, detail?.type, detail?.code, detail?.status,
    detail?.statusCode, detail?.status_code, detail?.httpStatusCode, detail?.codexErrorInfo,
    ...Object.keys(info ?? {}), ...Object.values(info ?? {}).map(value => object(value)?.httpStatusCode),
    record.status, record.statusCode, record.status_code, record.apiErrorStatus, record.apiErrorCode];
  if (fields.some(value => typeof value === "string" && (value.startsWith("oauth_") || value.startsWith("authentication_")))) return "authentication";
  if (fields.some(value => value === 401 || value === 403 || (typeof value === "string" && [
    "401", "403", "authentication_error", "auth", "unauthorized", "permission_error", "permission_denied",
    "forbidden", "subscription_disabled", "organization_disabled", "oauth_refresh_failed",
    "token_expired", "token_revoked", "refresh_token_reused", "refresh_token_expired", "refresh_token_invalidated",
    "logged_out", "signed_out", "unauthenticated", "authError",
  ].includes(value)))) return "authentication";
  if (fields.some(value => value === 429 || value === "429" || value === "rate_limit" || value === "rate_limit_error" || value === "usageLimitExceeded")) return "rate_limit";
  return "other";
}
