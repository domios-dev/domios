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
export function buildWeekBuckets(
  rows: UsageSnapshot[],
  cycleStart: number,
  cycleEnd: number,
  excludeWeekends: boolean,
): Bucket[] {
  const now = Date.now();
  const todayStart = localStartOfDay(now);
  const dayStart = localStartOfDay(cycleStart);
  const buckets: Bucket[] = [];
  for (let d = dayStart; d < cycleEnd; d += DAY) {
    const dayEnd = d + DAY;
    const inDay = rows.filter(
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

  const countDivisor = (slice: Bucket[]) =>
    excludeWeekends ? slice.filter((b) => !b.isWeekend).length : slice.length;

  const todayBucket = buckets.find((b) => b.current);
  if (todayBucket) {
    const fromTodayOnward = buckets.filter((b) => b.current || b.isFuture);
    const todayBudget = (() => {
      const divisor = countDivisor(fromTodayOnward);
      if (divisor === 0) return null;
      const remainingAtMidnight = Math.max(0, 100 - yesterdayEndCumulative);
      return remainingAtMidnight / divisor;
    })();
    todayBucket.target = excludeWeekends && todayBucket.isWeekend ? 0 : todayBudget;
  }

  // Future-day projections — recalculated from the *current* cumulative so any
  // over/under-spend today shifts the remaining days.
  const futureBuckets = buckets.filter((b) => b.isFuture);
  const futureDivisor = countDivisor(futureBuckets);
  const futureBudget = futureDivisor > 0 ? Math.max(0, 100 - currentCumulative) / futureDivisor : 0;
  for (const b of futureBuckets) {
    if (excludeWeekends && b.isWeekend) {
      b.pct = 0;
      b.target = 0;
    } else {
      b.pct = futureBudget;
      b.target = futureBudget;
    }
    b.projected = true;
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
  const withReset = rows.find((r) => r.weekly_resets_at != null);
  if (withReset && withReset.weekly_resets_at != null) {
    const end = withReset.weekly_resets_at;
    const start = end - 7 * DAY;
    return { start, end };
  }
  const now = Date.now();
  const d = new Date(now);
  const dayOfWeek = (d.getDay() + 6) % 7;
  const start = localStartOfDay(now - dayOfWeek * DAY);
  return { start, end: start + 7 * DAY };
}
