import { useEffect, useState } from "react";
import { invoke, listen } from "../../../lib/bridge";
import type { ProjectionResponse, WeeklyProjection } from "../../../hooks/useUsageSnapshot";
import type { UsageSnapshot } from "../../../types/usage";
import { activeWeeklyCycle, buildWeekBuckets } from "../../Usage/cycle";

const DAY = 24 * 60 * 60 * 1000;
const HISTORY_LIMIT = 500;

/** Share of the weekly allowance spent on one day of the current cycle; null for a day not yet lived. */
export interface AllowanceDay {
  start: number;
  label: string;
  pct: number | null;
  today: boolean;
}

/** Provider id to its current weekly cycle, day by day. */
export type AllowanceWeeks = Record<string, AllowanceDay[]>;

export function allowanceWeekDays(rows: UsageSnapshot[], weekly: WeeklyProjection | null): AllowanceDay[] {
  if (rows.length === 0 && !weekly) return [];
  const cycle = activeWeeklyCycle(rows);
  const buckets = buildWeekBuckets(
    rows,
    cycle.start,
    cycle.end,
    weekly
      ? {
          avgWeekdaySpend: weekly.avg_weekday_spend,
          avgWeekendSpend: weekly.avg_weekend_spend,
          dowEstimates: weekly.dow_estimates,
          restOfTodayEst: weekly.rest_of_today_est,
          dayActuals: weekly.day_actuals,
        }
      : null,
  );
  return buckets.map((bucket) => ({
    start: bucket.start,
    label: bucket.label,
    pct: bucket.isFuture ? null : bucket.pct,
    today: bucket.current,
  }));
}

async function readWeek(provider: string): Promise<AllowanceDay[]> {
  try {
    const [rows, projection] = await Promise.all([
      invoke<UsageSnapshot[] | null>("usage_history", { provider, since: Date.now() - 8 * DAY, limit: HISTORY_LIMIT }),
      invoke<ProjectionResponse | null>("usage_projection", { provider }),
    ]);
    return allowanceWeekDays(rows ?? [], projection?.weekly ?? null);
  } catch {
    return [];
  }
}

/** Reads each provider's allowance history for the default account and rereads when a new snapshot lands. */
export function useAllowanceWeeks(providers: readonly string[]): AllowanceWeeks {
  const key = providers.join("\n");
  const [weeks, setWeeks] = useState<AllowanceWeeks>({});
  const [revision, setRevision] = useState(0);

  useEffect(() => {
    if (!key) return;
    let active = true;
    void Promise.all(key.split("\n").map(async (provider) => [provider, await readWeek(provider)] as const)).then(
      (entries) => {
        if (active) setWeeks(Object.fromEntries(entries));
      },
    );
    return () => {
      active = false;
    };
  }, [key, revision]);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;
    listen("usage-snapshot-updated", () => setRevision((value) => value + 1))
      .then((fn) => {
        if (active) unlisten = fn;
        else fn();
      })
      .catch(() => undefined);
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  return weeks;
}
