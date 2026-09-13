import { useEffect, useState } from "react";
import { invoke, listen } from "../../../lib/bridge";
import type { UsageSnapshot } from "../../../types/usage";
import { activeWeeklyCycle, buildWeekBuckets } from "../../Usage/cycle";

const DAY = 24 * 60 * 60 * 1000;
const HISTORY_LIMIT = 500;

/** Share of the weekly allowance spent on one day of the current cycle; null for a day not yet lived or unobserved. */
export interface AllowanceDay {
  start: number;
  label: string;
  pct: number | null;
  today: boolean;
}

/** Provider id to its current weekly cycle, day by day. */
export type AllowanceWeeks = Record<string, AllowanceDay[]>;

// Built from raw snapshots rather than the backend projection's day_actuals, which match the cycle by exact reset time.
export function allowanceWeekDays(rows: UsageSnapshot[]): AllowanceDay[] {
  if (rows.length === 0) return [];
  const cycle = activeWeeklyCycle(rows);
  return buildWeekBuckets(rows, cycle.start, cycle.end, null).map((bucket) => ({
    start: bucket.start,
    label: new Date(bucket.start).toLocaleDateString("en-US", { weekday: "short" }),
    pct: bucket.isFuture ? null : bucket.pct,
    today: bucket.current,
  }));
}

async function readWeek(provider: string): Promise<AllowanceDay[]> {
  try {
    const rows = await invoke<UsageSnapshot[] | null>("usage_history", {
      provider,
      since: Date.now() - 8 * DAY,
      limit: HISTORY_LIMIT,
    });
    return allowanceWeekDays(rows ?? []);
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
