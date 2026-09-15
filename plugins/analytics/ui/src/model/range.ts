import type { AnalyticsBucket } from "@codeterm/plugin-sdk";
import { formatDay } from "./format";

export type RangePreset = "today" | "7d" | "30d" | "90d";

export const RANGE_OPTIONS: Array<{ value: RangePreset; label: string }> = [
  { value: "today", label: "Today" },
  { value: "7d", label: "7 days" },
  { value: "30d", label: "30 days" },
  { value: "90d", label: "90 days" },
];

const RANGE_DAYS: Record<RangePreset, number> = { today: 1, "7d": 7, "30d": 30, "90d": 90 };

/** The host caps day buckets at 31 days; longer ranges read in weeks at every width. */
export const MAX_DAY_BUCKET_DAYS = 31;

export interface LocalRange {
  from: string;
  to: string;
  bucket: AnalyticsBucket;
}

function pad(value: number): string {
  return String(value).padStart(2, "0");
}

export function localDay(date: Date): string {
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

export function parseLocalDay(day: string): Date {
  const [year, month, date] = day.split("-").map(Number);
  return new Date(year!, month! - 1, date!);
}

export function rangeFor(preset: RangePreset, now: Date): LocalRange {
  const days = RANGE_DAYS[preset];
  const from = new Date(now.getFullYear(), now.getMonth(), now.getDate() - (days - 1));
  return { from: localDay(from), to: localDay(now), bucket: days > MAX_DAY_BUCKET_DAYS ? "week" : "day" };
}

export function localTimeZone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
}

export function rangeCaption(range: { from: string; to: string }): string {
  const from = formatDay(parseLocalDay(range.from).getTime());
  if (range.from === range.to) return `${from} · local day`;
  return `${from} – ${formatDay(parseLocalDay(range.to).getTime())} · local days`;
}
