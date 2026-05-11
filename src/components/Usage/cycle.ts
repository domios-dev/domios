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

/**
 * Bucket the cycle into days. For each past day → actual delta from prior cumulative.
 * For today → actual delta so far + a `target` based on the budget that was set at
 * today's start (i.e. recalculated from yesterday's end-of-day cumulative).
 * For future days → `target` based on the current cumulative (so today's
 * over/under-spend ripples into the remaining days automatically).
 */
export interface BucketProjection {
  avgWeekdaySpend: number;
  avgWeekendSpend: number;
  /** Per day-of-week projected spend (0=Sun..6=Sat). Overrides avg* when present. */
  dowEstimates?: number[];
  restOfTodayEst: number;
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
  // Only samples in the CURRENT cycle count toward the bar's height — samples
  // from a previous cycle that happen to land on the same calendar day (cycle
  // boundary days like the start Sunday) would otherwise leak in.
  const cycleRows = rows.filter(
    (r) => r.weekly_resets_at == null || r.weekly_resets_at === cycleEnd,
  );
  const buckets: Bucket[] = [];
  for (let d = dayStart; d < cycleEnd; d += DAY) {
    const dayEnd = d + DAY;
    const inDay = cycleRows.filter(
      (r) => r.parsed_ok && r.captured_at >= d && r.captured_at < dayEnd && r.weekly_pct != null,
    );
    const label = new Date(d).toLocaleDateString(undefined, { weekday: "short" });
    const isToday = d === todayStart;
    const isFuture = d > todayStart;
    buckets.push({
      label,
      pct: inDay.length > 0 ? Math.max(...inDay.map((r) => r.weekly_pct!)) : null,
      target: null,
      projected: false,
      current: isToday,
      start: d,
      end: dayEnd,
      isWeekend: isWeekend(d),
      isFuture,
    });
  }

  // Cumulative → delta conversion. Walk forward, tracking the last known cumulative.
  // Past days where pct is null inherit prev (no spend recorded that day).
  let prev = 0;
  let yesterdayEndCumulative = 0;
  for (const b of buckets) {
    if (b.isFuture) continue;
    if (b.pct == null) {
      if (!b.current) b.pct = null;
      else b.pct = 0;
    } else {
      const cumulative = b.pct;
      b.pct = Math.max(0, cumulative - prev);
      prev = cumulative;
    }
    if (b.current) {
      // freeze "yesterday's end" so today's target uses the budget plan at midnight
      // — we already walked past, so prev now equals today's cumulative max. Subtract today's delta back.
      yesterdayEndCumulative = prev - (b.pct ?? 0);
    }
  }

  const currentCumulative = (() => {
    const past = rows
      .filter((r) => r.parsed_ok && r.weekly_pct != null && r.captured_at <= now)
      .sort((a, b) => b.captured_at - a.captured_at);
    return past[0]?.weekly_pct ?? prev;
  })();

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
      const remainingDays = buckets.filter((b) => b.current || b.isFuture).length;
      const remainingAtMidnight = Math.max(0, 100 - yesterdayEndCumulative);
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
    const futureBudget =
      futureBuckets.length > 0 ? Math.max(0, 100 - currentCumulative) / futureBuckets.length : 0;
    for (const b of futureBuckets) {
      b.pct = futureBudget;
      b.target = futureBudget;
      b.projected = true;
    }
  }

  return buckets;
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
  // Only trust a reset timestamp if it's still in the future. Stale rows from
  // a previous cycle would otherwise put us in a "cycle that already ended"
  // and future-day buckets would render off the chart's right edge.
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
