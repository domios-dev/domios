const TRANSIENT_PATTERNS = [
  /\btypeerror:/i,
  /load failed/i,
  /failed to fetch/i,
  /networkerror/i,
  /network request failed/i,
  /the internet connection appears to be offline/i,
  /\b(?:502|503|504)\b/,
];

const MAX_ERROR_DETAIL_LENGTH = 240;

function sanitizeErrorDetail(raw: string): string {
  const firstLine = raw
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find((line) => line.length > 0 && !/^at\s+\S+/.test(line)) ?? "";
  if (!firstLine) return "";
  const cleaned = firstLine
    .replace(/[\u0000-\u001f\u007f]+/g, " ")
    .replace(/\s+/g, " ")
    .replace(/\b(Bearer|Basic)\s+[A-Za-z0-9._~+/=-]+/gi, "$1 [redacted]")
    .replace(/(https?:\/\/)([^/\s:@]+:[^/\s@]+)@/gi, "$1[redacted]@")
    .replace(
      /([?&](?:token|access_token|refresh_token|id_token|api_key|apikey|key|secret|password|auth)=)[^&\s#]+/gi,
      "$1[redacted]",
    )
    .replace(
      /(\b(?:authorization|x-api-key|api[_-]?key|access[_-]?token|refresh[_-]?token|id[_-]?token|token|secret|password|cookie|set-cookie)\b\s*[:=]\s*)("[^"]*"|'[^']*'|[^\s,&;})]+)/gi,
      "$1[redacted]",
    )
    .trim();
  if (cleaned.length <= MAX_ERROR_DETAIL_LENGTH) return cleaned;
  return `${cleaned.slice(0, MAX_ERROR_DETAIL_LENGTH - 1).trimEnd()}…`;
}

export function isTransientFetchError(err: unknown): boolean {
  if (err instanceof Error && err.name === "AbortError") return false;
  const message = err instanceof Error ? err.message : String(err);
  if (!message) return true;
  return TRANSIENT_PATTERNS.some((re) => re.test(message));
}

export function formatPlanFetchError(err: unknown): string {
  if (isTransientFetchError(err)) {
    return "Could not reach Domios. Reconnecting…";
  }
  const message = err instanceof Error ? err.message : typeof err === "string" ? err : err == null ? "" : String(err);
  const detail = sanitizeErrorDetail(message);
  if (!detail) {
    return "Could not load plans. Try again.";
  }
  if (/^\d{3}\b/.test(detail) || detail.includes("failed:")) {
    return detail;
  }
  return `Could not load plans. Try again. (${detail})`;
}

export function computeBackoffDelay(
  attempt: number,
  initialMs = 1_000,
  maxMs = 15_000,
): number {
  const base = Math.min(initialMs * 2 ** Math.max(0, attempt - 1), maxMs);
  return base / 2 + Math.random() * (base / 2);
}
