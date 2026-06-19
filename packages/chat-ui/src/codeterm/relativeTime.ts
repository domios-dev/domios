// Humanize an epoch-ms timestamp (string or number) into a short relative
// label like "5m ago" / "in 2h" / "just now". Returns null for unparseable
// input so callers can fall back to omitting the affordance.
export function relativeTime(value: string | number | undefined, now: number = Date.now()): string | null {
  if (value == null) return null;
  const ms = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(ms)) return null;
  const deltaSec = Math.round((ms - now) / 1000);
  const past = deltaSec <= 0;
  const abs = Math.abs(deltaSec);
  if (abs < 45) return "just now";
  let amount: number;
  let unit: string;
  if (abs < 3600) {
    amount = Math.round(abs / 60);
    unit = "m";
  } else if (abs < 86400) {
    amount = Math.round(abs / 3600);
    unit = "h";
  } else {
    amount = Math.round(abs / 86400);
    unit = "d";
  }
  return past ? `${amount}${unit} ago` : `in ${amount}${unit}`;
}

// Absolute local timestamp for the hover/title affordance.
export function absoluteTime(value: string | number | undefined): string | null {
  if (value == null) return null;
  const ms = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(ms)) return null;
  return new Date(ms).toLocaleString();
}

// Per-message timestamp (ISO string from the provider JSONL) → short local
// time for display plus a full absolute label for the title attribute.
export function messageTimestamp(value: string | undefined): { short: string; full: string } | null {
  if (!value) return null;
  const date = new Date(value);
  const ms = date.getTime();
  if (!Number.isFinite(ms)) return null;
  return {
    short: date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }),
    full: date.toLocaleString(),
  };
}
