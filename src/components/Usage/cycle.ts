import type { UsageSnapshot } from "../../types/usage";

export type CycleRange = "week" | "month" | "year";

export interface Bucket {
  label: string;
  /** Actual delta spent this day (cumulative_end - cumulative_start). Null until we have data. */
  pct: number | null;
  /** Planned budget for this day. Set for today + future days, null for past. */
  target: number | null;
  /** True for future-only bars (dashed outline, no fill). */
  projected: boolean;
  /** True for the in-progress day — filled bar + dashed outline overlay. */
  current: boolean;
  start: number;
  end: number;
  isWeekend?: boolean;
  isFuture?: boolean;
}

const DAY = 24 * 60 * 60 * 1000;

/** Index of the first bucket whose cumulative end-of-day total crosses 100%.
 * Past day actuals + today/future targets. Returns -1 when the projection
 * stays under 100% through the whole cycle. */
export function findOverspendIndex(buckets: Bucket[]): number {
  let cum = 0;
  for (let i = 0; i < buckets.length; i++) {
    const b = buckets[i];
    const day =
      b.isFuture || b.current
        ? (b.target ?? b.pct ?? 0)
        : (b.pct ?? 0);
    cum += day;
    if (cum > 100) return i;
  }
  return -1;
}

/** Cumulative end-of-day total per bucket. */
export function cumByBucket(buckets: Bucket[]): number[] {
  let cum = 0;
  return buckets.map((b) => {
    const day =
      b.isFuture || b.current
        ? (b.target ?? b.pct ?? 0)
        : (b.pct ?? 0);
    cum += day;
    return cum;
  });
}

function localStartOfDay(ms: number): number {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

function isWeekend(ms: number): boolean {
  const day = new Date(ms).getDay();
  return day === 0 || day === 6;
}

export interface DayActualInput {
  day_start_ms: number;
  pct: number | null;
}

export interface BucketProjection {
  avgWeekdaySpend: number;
  avgWeekendSpend: number;
  dowEstimates?: number[];
  restOfTodayEst: number;
  dayActuals?: DayActualInput[];
}

export function buildWeekBuckets(
  rows: UsageSnapshot[],
  cycleStart: number,
  cycleEnd: number,
  projection: BucketProjection | null,
): Bucket[] {
  const now = Date.now();
  const todayStart = localStartOfDay(now);
  const dayStart = localStartOfDay(cycleStart);

  const actualByDay = new Map<number, number | null>();
  if (projection?.dayActuals) {
    for (const a of projection.dayActuals) {
      actualByDay.set(localStartOfDay(a.day_start_ms), a.pct);
    }
  } else {
    fallbackDeltas(rows, cycleEnd, dayStart, todayStart, actualByDay);
  }

  const buckets: Bucket[] = [];
  for (let d = dayStart; d < cycleEnd; d += DAY) {
    const dayEnd = d + DAY;
    const label = new Date(d).toLocaleDateString(undefined, { weekday: "short" });
    const isToday = d === todayStart;
    const isFuture = d > todayStart;
    const pct = isFuture ? null : (actualByDay.get(d) ?? null);
    buckets.push({
      label,
      pct,
      target: null,
      projected: false,
      current: isToday,
      start: d,
      end: dayEnd,
      isWeekend: isWeekend(d),
      isFuture,
    });
  }

  const dayBaseline = (b: Bucket): number => {
    if (!projection) return 0;
    if (projection.dowEstimates && projection.dowEstimates.length === 7) {
      const dow = new Date(b.start).getDay();
      return projection.dowEstimates[dow];
    }
    return b.isWeekend ? projection.avgWeekendSpend : projection.avgWeekdaySpend;
  };

  const todayBucket = buckets.find((b) => b.current);
  if (todayBucket) {
    if (projection) {
      const todaySoFar = todayBucket.pct ?? 0;
      todayBucket.target = todaySoFar + Math.max(0, projection.restOfTodayEst);
    } else {
      const pastCum = buckets
        .filter((b) => !b.current && !b.isFuture)
        .reduce((sum, b) => sum + (b.pct ?? 0), 0);
      const remainingDays = buckets.filter((b) => b.current || b.isFuture).length;
      const remainingAtMidnight = Math.max(0, 100 - pastCum);
      todayBucket.target = remainingDays > 0 ? remainingAtMidnight / remainingDays : null;
    }
  }

  const futureBuckets = buckets.filter((b) => b.isFuture);
  if (projection) {
    for (const b of futureBuckets) {
      const v = dayBaseline(b);
      b.pct = v;
      b.target = v;
      b.projected = true;
    }
  } else {
    const allKnownCum = buckets
      .filter((b) => !b.isFuture)
      .reduce((sum, b) => sum + (b.pct ?? 0), 0);
    const futureBudget =
      futureBuckets.length > 0 ? Math.max(0, 100 - allKnownCum) / futureBuckets.length : 0;
    for (const b of futureBuckets) {
      b.pct = futureBudget;
      b.target = futureBudget;
      b.projected = true;
    }
  }

  return buckets;
}

// Mirrors backend day_delta_for_window — used until the projection lands.
function fallbackDeltas(
  rows: UsageSnapshot[],
  cycleEnd: number,
  dayStart: number,
  todayStart: number,
  out: Map<number, number | null>,
) {
  const cycleRows = rows
    .filter((r) => r.parsed_ok && r.weekly_pct != null)
    .filter((r) => r.weekly_resets_at == null || r.weekly_resets_at === cycleEnd)
    .sort((a, b) => a.captured_at - b.captured_at);
  const MAX_BRIDGE_GAP_MS = 2 * 60 * 60 * 1000;
  for (let d = dayStart; d < cycleEnd; d += DAY) {
    if (d > todayStart) break;
    const dayEnd = d + DAY;
    const inDay = cycleRows.filter((r) => r.captured_at >= d && r.captured_at < dayEnd);
    if (inDay.length === 0) {
      out.set(d, null);
      continue;
    }
    let total = 0;
    const prevBefore = cycleRows.filter((r) => r.captured_at < d).pop();
    if (prevBefore && d - prevBefore.captured_at <= MAX_BRIDGE_GAP_MS) {
      const first = inDay[0].weekly_pct!;
      if (prevBefore.weekly_pct! <= first) total += first - prevBefore.weekly_pct!;
    }
    for (let i = 1; i < inDay.length; i++) {
      const delta = inDay[i].weekly_pct! - inDay[i - 1].weekly_pct!;
      if (delta > 0) total += delta;
    }
    out.set(d, total);
  }
}

export function buildMonthBuckets(rows: UsageSnapshot[], now: number): Bucket[] {
  const monthStart = new Date(now);
  monthStart.setDate(1);
  monthStart.setHours(0, 0, 0, 0);
  const buckets: Bucket[] = [];
  let weekStart = monthStart.getTime();
  while (true) {
    const weekEnd = weekStart + 7 * DAY;
    const inRange = rows.filter(
      (r) => r.parsed_ok && r.captured_at >= weekStart && r.captured_at < weekEnd && r.weekly_pct != null,
    );
    const peak = inRange.length > 0 ? Math.max(...inRange.map((r) => r.weekly_pct!)) : null;
    const label = `W${Math.floor((weekStart - monthStart.getTime()) / (7 * DAY)) + 1}`;
    buckets.push({
      label,
      pct: peak,
      target: null,
      projected: false,
      current: false,
      start: weekStart,
      end: weekEnd,
      isFuture: weekStart > now,
    });
    weekStart = weekEnd;
    if (weekStart > now + 7 * DAY) break;
    if (buckets.length >= 6) break;
  }
  return buckets;
}

export function buildYearBuckets(rows: UsageSnapshot[], now: number): Bucket[] {
  const year = new Date(now).getFullYear();
  const buckets: Bucket[] = [];
  for (let m = 0; m < 12; m++) {
    const monthStart = new Date(year, m, 1).getTime();
    const monthEnd = new Date(year, m + 1, 1).getTime();
    const inRange = rows.filter(
      (r) => r.parsed_ok && r.captured_at >= monthStart && r.captured_at < monthEnd && r.weekly_pct != null,
    );
    const peak = inRange.length > 0 ? Math.max(...inRange.map((r) => r.weekly_pct!)) : null;
    const label = new Date(year, m, 1).toLocaleDateString(undefined, { month: "short" });
    buckets.push({
      label,
      pct: peak,
      target: null,
      projected: false,
      current: false,
      start: monthStart,
      end: monthEnd,
      isFuture: monthStart > now,
    });
  }
  return buckets;
}

export function activeWeeklyCycle(rows: UsageSnapshot[]): { start: number; end: number } {
  const now = Date.now();
  // Only trust a reset timestamp if it's still in the future. Stale rows from a previous cycle would otherwise put us in a "cycle that already
  // ended" and future-day buckets would render off the chart's right edge.
  const withReset = rows.find(
    (r) => r.weekly_resets_at != null && r.weekly_resets_at > now,
  );
  if (withReset && withReset.weekly_resets_at != null) {
    const end = withReset.weekly_resets_at;
    const start = end - 7 * DAY;
    return { start, end };
  }
  const d = new Date(now);
  const dayOfWeek = (d.getDay() + 6) % 7;
  const start = localStartOfDay(now - dayOfWeek * DAY);
  return { start, end: start + 7 * DAY };
}
