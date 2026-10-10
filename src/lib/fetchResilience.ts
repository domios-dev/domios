const TRANSIENT_PATTERNS = [
  /^typeerror:/i,
  /load failed/i,
  /failed to fetch/i,
  /networkerror/i,
  /network request failed/i,
  /the internet connection appears to be offline/i,
  /\b(?:502|503|504)\b/,
];

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
  const message = err instanceof Error ? err.message : String(err);
  if (/^\d{3}\b/.test(message) || message.includes("failed:")) {
    return message;
  }
  return "Could not load plans. Try again.";
}

export function computeBackoffDelay(
  attempt: number,
  initialMs = 1_000,
  maxMs = 15_000,
): number {
  const base = Math.min(initialMs * 2 ** Math.max(0, attempt - 1), maxMs);
  return base / 2 + Math.random() * (base / 2);
}
