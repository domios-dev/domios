const SECOND = 1000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const MONTH = 30 * DAY;
const YEAR = 365 * DAY;

/** The largest whole unit of a span: "45s", "12m", "3h", "2d", "4mo", "1y". Only beside a label that says what it measures. */
export function formatSpan(ms: number): string {
  const span = Math.max(0, ms);
  if (span < MINUTE) return `${Math.floor(span / SECOND)}s`;
  if (span < HOUR) return `${Math.floor(span / MINUTE)}m`;
  if (span < DAY) return `${Math.floor(span / HOUR)}h`;
  if (span < MONTH) return `${Math.floor(span / DAY)}d`;
  if (span < YEAR) return `${Math.floor(span / MONTH)}mo`;
  return `${Math.floor(span / YEAR)}y`;
}

/** How long something ran: "45s", "3m 10s", "1h 5m", "2d 3h". */
export function formatDuration(ms: number): string {
  const span = Math.max(0, ms);
  if (span < MINUTE) return `${Math.floor(span / SECOND)}s`;
  const [major, minor, majorUnit, minorUnit] = span < HOUR
    ? [Math.floor(span / MINUTE), Math.floor((span % MINUTE) / SECOND), "m", "s"]
    : span < DAY
      ? [Math.floor(span / HOUR), Math.floor((span % HOUR) / MINUTE), "h", "m"]
      : [Math.floor(span / DAY), Math.floor((span % DAY) / HOUR), "d", "h"];
  return minor > 0 || majorUnit === "m" ? `${major}${majorUnit} ${minor}${minorUnit}` : `${major}${majorUnit}`;
}

/** How long ago: "just now", "12m ago", "2d ago". */
export function formatAge(ts: number, now: number): string {
  const age = now - ts;
  return age < MINUTE ? "just now" : `${formatSpan(age)} ago`;
}

/** How long until: "in <1m", "in 12m", "in 2d"; a moment already passed reads as an age. */
export function formatUntil(ts: number, now: number): string {
  const wait = ts - now;
  if (wait < 0) return formatAge(ts, now);
  return wait < MINUTE ? "in <1m" : `in ${formatSpan(wait)}`;
}
