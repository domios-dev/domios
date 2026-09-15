import type { AnalyticsSnapshot, AnalyticsTokenBucket, AnalyticsTotals } from "@codeterm/plugin-sdk";
import type {
  SpendChartData,
  SpendChartDay,
  SpendChartSegment,
  SpendChartSeries,
  SpendChartTone,
} from "../../../../../src/components/Canvas/missionControl/spendChart";
import { formatClock } from "./format";
import { chartKey, keyId, keyLabel, toneFor } from "./keys";

export type Measure = "new_input" | "generated" | "turns";

export const MEASURE_OPTIONS: Array<{ value: Measure; label: string }> = [
  { value: "new_input", label: "New input" },
  { value: "generated", label: "Generated" },
  { value: "turns", label: "Turns" },
];

export function measureLabel(measure: Measure): string {
  return MEASURE_OPTIONS.find((option) => option.value === measure)!.label;
}

export function measureOf(totals: AnalyticsTotals, measure: Measure): number {
  return totals[measure];
}

const MONTH_DAY = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric" });
const WEEKDAY = new Intl.DateTimeFormat("en-US", { weekday: "short" });

function toneRank(tone: SpendChartTone): number {
  if (typeof tone === "number") return tone;
  return tone === "other" ? 10 : 11;
}

export function isToday(bucket: { start: number; end: number }, nowMs: number): boolean {
  return nowMs >= bucket.start * 1000 && nowMs < bucket.end * 1000;
}

export function bucketLabel(bucket: AnalyticsTokenBucket, weekly: boolean, compact: boolean): string {
  const date = new Date(bucket.start * 1000);
  if (weekly) return compact ? `${date.getMonth() + 1}/${date.getDate()}` : MONTH_DAY.format(date);
  return compact ? String(date.getDate()) : `${WEEKDAY.format(date)} ${date.getDate()}`;
}

function bucketNote(snapshot: AnalyticsSnapshot, bucket: AnalyticsTokenBucket, nowMs: number, weekly: boolean): string | undefined {
  if (bucket.coverage === "not_recorded") return "not recorded";
  if (isToday(bucket, nowMs)) return weekly ? "so far" : "today";
  const since = snapshot.recording.first_recorded_at;
  if (bucket.coverage === "partial" && since !== null && since >= bucket.start && since < bucket.end) {
    return `from ${formatClock(since * 1000)}`;
  }
  return undefined;
}

export function chartData(
  snapshot: AnalyticsSnapshot,
  measure: Measure,
  tones: ReadonlyMap<string, SpendChartTone>,
  labels: ReadonlyMap<string, string>,
  nowMs: number,
  compact: boolean,
): SpendChartData {
  const weekly = snapshot.bucket === "week";
  const seen = new Map<string, SpendChartSeries>();
  const days = snapshot.tokens.series.map<SpendChartDay>((bucket) => {
    const segments = new Map<string, SpendChartSegment>();
    for (const group of bucket.groups) {
      const key = chartKey(group.key);
      const tone = toneFor(tones, key);
      const id = tone === "other" ? "other" : keyId(key);
      const label = tone === "other" ? "Other" : labels.get(id) ?? keyLabel(key, null);
      const segment = segments.get(id) ?? { key: id, label, tone, value: 0 };
      segment.value += measureOf(group.totals, measure);
      segments.set(id, segment);
    }
    const kept = [...segments.values()].filter((segment) => segment.value > 0).sort((a, b) => toneRank(a.tone) - toneRank(b.tone));
    for (const segment of kept) seen.set(segment.key, { key: segment.key, label: segment.label, tone: segment.tone });
    return {
      start: bucket.start * 1000,
      label: bucketLabel(bucket, weekly, compact),
      today: isToday(bucket, nowMs),
      segments: kept,
      total: bucket.totals ? measureOf(bucket.totals, measure) : 0,
      coverage: bucket.coverage,
      note: compact ? undefined : bucketNote(snapshot, bucket, nowMs, weekly),
    };
  });
  const legend = [...seen.values()].sort((a, b) => toneRank(a.tone) - toneRank(b.tone));
  if (snapshot.tokens.series.some((bucket) => bucket.coverage === "not_recorded")) {
    legend.push({ key: "not_recorded", label: "not recorded, no figure exists", tone: "not_recorded" });
  }
  return { days, legend };
}

export interface BucketReading {
  label: string;
  coverage: AnalyticsTokenBucket["coverage"];
  value: number | null;
  turns: number | null;
  segments: SpendChartSegment[];
}

export function bucketReading(data: SpendChartData, snapshot: AnalyticsSnapshot, startMs: number, measure: Measure): BucketReading | null {
  const index = data.days.findIndex((day) => day.start === startMs);
  const bucket = snapshot.tokens.series[index];
  const day = data.days[index];
  if (!bucket || !day) return null;
  return {
    label: `${day.label}${day.note ? ` · ${day.note}` : ""}`,
    coverage: bucket.coverage,
    value: bucket.totals ? measureOf(bucket.totals, measure) : null,
    turns: bucket.totals ? bucket.totals.turns : null,
    segments: day.segments,
  };
}
