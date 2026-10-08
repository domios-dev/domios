const UNITS = [
  { limit: 1_000_000_000, suffix: "B" },
  { limit: 1_000_000, suffix: "M" },
  { limit: 1_000, suffix: "k" },
] as const;

/** 999, 1k, 1.1k, 9.9k, 10k, 999k, 1M, 1.2M: one decimal below ten units, none from ten up. */
export function formatCompact(value: number): string {
  if (!Number.isFinite(value)) return "—";
  const abs = Math.abs(value);
  const sign = value < 0 ? "-" : "";
  if (Math.round(abs) < 1_000) return `${sign}${Math.round(abs)}`;
  for (let index = UNITS.length - 1; index >= 0; index -= 1) {
    const unit = UNITS[index];
    const larger = UNITS[index - 1];
    if (larger && abs >= larger.limit) continue;
    const scaled = abs / unit.limit;
    const digits = scaled >= 10 ? 0 : 1;
    const text = scaled.toFixed(digits).replace(/\.0$/, "");
    if (larger && Number(text) >= 1_000) return `${sign}${formatCompact(larger.limit).replace(/^-/, "")}`;
    return `${sign}${text}${unit.suffix}`;
  }
  return `${sign}${Math.round(abs)}`;
}

/** The exact value for a tooltip next to a compact count. */
export function formatExact(value: number): string {
  return Math.round(value).toLocaleString("en-US");
}
