/** Describe a cron schedule string in human-readable form. */
export function describeSchedule(cron: string): string {
  const parts = cron.trim().split(/\s+/);
  if (parts.length !== 5) return cron;
  const [min, hour, dom, mon, dow] = parts;
  if (min === "*" && hour === "*") return "Every minute";
  if (hour === "*" && dom === "*" && mon === "*" && dow === "*") {
    if (min === "0") return "Every hour";
    if (min.startsWith("*/")) return `Every ${min.slice(2)} minutes`;
  }
  if (dom === "*" && mon === "*" && dow === "*") {
    if (hour.startsWith("*/")) return `Every ${hour.slice(2)} hours at :${min.padStart(2, "0")}`;
    return `Daily at ${hour}:${min.padStart(2, "0")}`;
  }
  if (dom === "*" && mon === "*" && dow !== "*") {
    const days = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const day = days[parseInt(dow)] || dow;
    return `Every ${day} at ${hour}:${min.padStart(2, "0")}`;
  }
  return cron;
}

/** Relative time from a timestamp (ms) — "just now", "5m ago", "2h ago", "3d ago". */
export function timeAgo(ts: number): string {
  const diff = Date.now() - ts;
  if (diff < 60_000) return "just now";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
  return `${Math.floor(diff / 86_400_000)}d ago`;
}

/** Format a timestamp for display — "Apr 2, 10:30 AM". */
export function formatTime(ms: number): string {
  return new Date(ms).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}
