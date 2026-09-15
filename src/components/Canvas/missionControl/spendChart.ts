/** A numbered series tone, or a neutral one: folded "other" keys, keys with no name, and buckets never recorded. */
export type SpendChartTone = number | "other" | "unnamed" | "not_recorded";

export interface SpendChartSeries {
  key: string;
  label: string;
  tone: SpendChartTone;
}

export interface SpendChartSegment extends SpendChartSeries {
  value: number;
}

export type SpendChartCoverage = "recorded" | "partial" | "not_recorded";

export interface SpendChartDay {
  start: number;
  label: string;
  today: boolean;
  segments: SpendChartSegment[];
  total: number;
  /** Absent means recorded. A not-recorded bar carries no figure. */
  coverage?: SpendChartCoverage;
  /** A short second line under the label, such as when recording began. */
  note?: string;
}

/** What the spend chart draws: a bar per day, and a legend only when the bars are split. */
export interface SpendChartData {
  days: SpendChartDay[];
  legend: SpendChartSeries[];
}

export function spendChartToneAttr(tone: SpendChartTone): string {
  return typeof tone === "number" ? String(tone % 4) : tone;
}

export function startOfToday(): number {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  return today.getTime();
}

export function dayLabel(start: number): string {
  return new Date(start).toLocaleDateString("en-US", { weekday: "short" });
}
