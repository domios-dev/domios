/** Epoch-ms floor (2001-09-09). A live-chat timestamp in milliseconds is always
 *  above this; a Unix-SECONDS stamp for any plausible year is below it (2026 ≈
 *  1.78e9). Providers are NOT consistent about the unit — claude/opencode hand
 *  the frontend millisecond values, but codex computes `Math.floor(date/1000)`
 *  and `RichMessage.ts` is documented in Unix seconds. If a seconds value ever
 *  reaches the optimistic-echo matcher it would be read as 1970 — ~56 years
 *  before the bubble's send time — so the skew guard rejects every real echo
 *  and the bubble sticks on "sending…". Normalizing by magnitude makes the
 *  matcher unit-agnostic without weakening the stale-echo guard (a genuinely
 *  old stamp scales to a genuinely old ms value, still rejected). */
const MS_EPOCH_FLOOR = 1e12;

/** Parse a chat timestamp (numeric epoch string OR ISO-8601) to epoch ms,
 *  scaling Unix-seconds values up to ms. Returns null when absent/unparseable. */
export function timestampMs(ts: string | number | undefined | null): number | null {
  if (ts == null || ts === "") return null;
  const numeric = typeof ts === "number" ? ts : Number(ts);
  if (Number.isFinite(numeric)) {
    if (numeric <= 0) return null;
    return numeric < MS_EPOCH_FLOOR ? numeric * 1000 : numeric;
  }
  const parsed = Date.parse(String(ts));
  return Number.isNaN(parsed) ? null : parsed;
}
