import { formatAge, formatUntil } from "./timeLabels";

/** Common cron schedule presets for UI selectors. */
export const SCHEDULE_PRESETS = [
  { label: "Every minute", value: "* * * * *" },
  { label: "Every 5 min", value: "*/5 * * * *" },
  { label: "Every hour", value: "0 * * * *" },
  { label: "Every 6h", value: "0 */6 * * *" },
  { label: "Daily 9 AM", value: "0 9 * * *" },
  { label: "Weekly Mon 9 AM", value: "0 9 * * 1" },
] as const;

const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

function wholeNumberInRange(value: string, min: number, max: number): boolean {
  if (!/^\d+$/.test(value)) return false;
  const n = parseInt(value, 10);
  return n >= min && n <= max;
}

function stepValue(value: string, max: number): number | null {
  if (!value.startsWith("*/")) return null;
  const raw = value.slice(2);
  if (!/^\d+$/.test(raw)) return null;
  const n = parseInt(raw, 10);
  return n >= 1 && n <= max ? n : null;
}

/** Describe a cron schedule string in human-readable form. */
export function describeSchedule(cron: string): string {
  const parts = cron.trim().split(/\s+/);
  if (parts.length !== 5) return cron;
  const [min, hour, dom, mon, dow] = parts;

  if (min === "*" && hour === "*" && dom === "*" && mon === "*" && dow === "*") {
    return "Every minute";
  }

  if (dom === "*" && mon === "*" && dow === "*") {
    const minStep = stepValue(min, 59);
    if (minStep !== null && hour === "*") return `Every ${minStep} minutes`;
    if (min === "0" && hour === "*") return "Every hour";

    const hourStep = stepValue(hour, 23);
    if (hourStep !== null && wholeNumberInRange(min, 0, 59)) {
      return `Every ${hourStep} hours at :${min.padStart(2, "0")}`;
    }
    if (wholeNumberInRange(hour, 0, 23) && wholeNumberInRange(min, 0, 59)) {
      return `Daily at ${hour}:${min.padStart(2, "0")}`;
    }
    return cron;
  }

  if (dom === "*" && mon === "*" && dow !== "*") {
    if (wholeNumberInRange(dow, 0, 6) && wholeNumberInRange(hour, 0, 23) && wholeNumberInRange(min, 0, 59)) {
      return `Every ${WEEKDAYS[parseInt(dow, 10)]} at ${hour}:${min.padStart(2, "0")}`;
    }
    return cron;
  }

  return cron;
}

export function timeAgo(ts: number): string {
  return formatAge(ts, Date.now());
}

/** Format a timestamp for display — "Apr 2, 10:30 AM". */
export function formatTime(ms: number): string {
  return new Date(ms).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

/** Relative future time — "in 5m", "in 2h", "tomorrow 14:30", or absolute date. */
export function formatFuture(ms: number): string {
  const diff = ms - Date.now();
  if (diff < 0) return "overdue";
  if (diff < 86_400_000) return formatUntil(ms, Date.now());
  if (diff < 2 * 86_400_000) {
    const d = new Date(ms);
    return `tomorrow ${d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`;
  }
  return formatTime(ms);
}
