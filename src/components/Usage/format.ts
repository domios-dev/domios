export function formatRelative(now: number, then: number): string {
  const diff = Math.max(0, Math.floor((now - then) / 1000));
  if (diff < 60) return `${diff}s ago`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

export function formatResetsAt(ms: number, now: number): string {
  const d = new Date(ms);
  const sameDay = new Date(now).toDateString() === d.toDateString();
  const time = d.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
  if (sameDay) {
    const remain = Math.max(0, ms - now);
    const h = Math.floor(remain / 3_600_000);
    const m = Math.floor((remain % 3_600_000) / 60_000);
    return h > 0 ? `resets ${time} · in ${h}h ${m}m` : `resets ${time} · in ${m}m`;
  }
  const date = d.toLocaleDateString("en-US", { month: "short", day: "numeric" });
  return `resets ${date} · ${time}`;
}

export function formatSpend(cents: number): string {
  const dollars = cents / 100;
  return dollars >= 100 ? `$${Math.round(dollars)}` : `$${dollars.toFixed(2)}`;
}

export function formatStampShort(ms: number): string {
  const d = new Date(ms);
  return d.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}
