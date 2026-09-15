import { useCallback, useEffect, useRef, useState } from "react";
import type { AnalyticsGroupBy } from "@codeterm/plugin-sdk";
import { createRefreshGate, type RefreshGate } from "../../../../src/lib/refreshGate";
import type { QueryOutcome } from "../../src/plugin";
import { onHostNotice, queryAnalytics } from "./bridge";
import type { Catalog } from "./model/options";
import { activeFilterCount, buildQuery, GROUP_BY_OPTIONS, NO_FILTERS, type FilterState } from "./model/query";
import { localTimeZone, rangeFor, type LocalRange, type RangePreset } from "./model/range";

export interface AnalyticsRequest {
  preset: RangePreset;
  groupBy: AnalyticsGroupBy;
  filters: FilterState;
}

export interface AnalyticsData {
  outcome: QueryOutcome | null;
  catalog: Catalog;
  nowMs: number;
  range: LocalRange;
  refreshing: boolean;
  refresh: () => void;
}

interface Loaded {
  outcome: QueryOutcome | null;
  catalog: Catalog;
  nowMs: number;
  range: LocalRange;
}

/**
 * Unfiltered readings per grouping give filter choices and stable tones; a filtered reading is fetched only
 * when a filter is set. Figures already on screen stay until the newer answer lands, and a stale answer is dropped.
 */
export function useAnalytics({ preset, groupBy, filters }: AnalyticsRequest): AnalyticsData {
  const [nonce, setNonce] = useState(0);
  const [refreshing, setRefreshing] = useState(true);
  const [loaded, setLoaded] = useState<Loaded>(() => ({
    outcome: null,
    catalog: {},
    nowMs: Date.now(),
    range: rangeFor(preset, new Date()),
  }));
  const sequence = useRef(0);
  const gate = useRef<RefreshGate | null>(null);
  const filtersRef = useRef(filters);
  filtersRef.current = filters;
  const filtersKey = JSON.stringify(filters);

  useEffect(() => {
    const id = ++sequence.current;
    const now = new Date();
    const range = rangeFor(preset, now);
    const tz = localTimeZone();
    const current = filtersRef.current;
    const groupBys = GROUP_BY_OPTIONS.map((option) => option.value);
    setRefreshing(true);
    void Promise.all([
      Promise.all(groupBys.map((group) => queryAnalytics(buildQuery(range, tz, group, NO_FILTERS)))),
      activeFilterCount(current) > 0 ? queryAnalytics(buildQuery(range, tz, groupBy, current)) : Promise.resolve(null),
    ]).then(([unfiltered, filtered]) => {
      if (id !== sequence.current) return;
      const catalog: Catalog = {};
      groupBys.forEach((group, index) => {
        const outcome = unfiltered[index];
        if (outcome?.kind === "snapshot") catalog[group] = outcome.snapshot;
      });
      setLoaded({ outcome: filtered ?? unfiltered[groupBys.indexOf(groupBy)] ?? null, catalog, nowMs: now.getTime(), range });
      setRefreshing(false);
      gate.current?.settled();
    });
  }, [preset, groupBy, filtersKey, nonce]);

  const refresh = useCallback(() => setNonce((value) => value + 1), []);

  // KPI notices re-read only while shown; being shown again after a hide is the single catch-up read.
  useEffect(() => {
    const freshness = createRefreshGate(refresh);
    gate.current = freshness;
    let shown = true;
    const stop = onHostNotice((notice) => {
      if (notice === "hidden") {
        shown = false;
        freshness.cancel();
        return;
      }
      if (notice === "shown") {
        if (shown) return;
        shown = true;
      } else if (!shown) {
        return;
      }
      freshness.request();
    });
    return () => {
      stop();
      freshness.cancel();
      if (gate.current === freshness) gate.current = null;
    };
  }, [refresh]);

  return { ...loaded, refreshing, refresh };
}
