import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "../../lib/bridge";
import { RefreshCw, X, LineChart, Table as TableIcon } from "lucide-react";
import { classify, type UsageSnapshot } from "../../types/usage";
import { useUsageHistory, useUsageProjection } from "../../hooks/useUsageSnapshot";
import { formatRelative } from "./format";
import { UsageChart, type Range } from "./UsageChart";
import { UsageTable } from "./UsageTable";
import {
  activeWeeklyCycle,
  buildMonthBuckets,
  buildWeekBuckets,
  buildYearBuckets,
} from "./cycle";
import { getAgentDescriptor } from "../../lib/agentDescriptorCache";

interface UsageHistoryModalProps {
  provider: string;
  accountId: string | null;
  onClose: () => void;
}

const RANGE_MS: Record<Range, number> = {
  week: 8 * 24 * 60 * 60 * 1000,
  month: 32 * 24 * 60 * 60 * 1000,
  year: 366 * 24 * 60 * 60 * 1000,
};

const REFRESH_TIMEOUT_MS = 12_000;

export function UsageHistoryModal({ provider, accountId, onClose }: UsageHistoryModalProps) {
  const [range, setRange] = useState<Range>("week");
  const [refreshing, setRefreshing] = useState(false);
  const refreshStartRef = useRef<number>(0);
  const since = useMemo(() => Date.now() - RANGE_MS[range], [range]);
  const limit = range === "week" ? 500 : range === "month" ? 2000 : 8000;
  const { rows } = useUsageHistory({ provider, accountId, sinceMs: since, limit });
  const { projection } = useUsageProjection({ provider, accountId });

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  useEffect(() => {
    if (!refreshing) return;
    const latest = rows[0]?.captured_at ?? 0;
    if (latest > refreshStartRef.current) {
      setRefreshing(false);
      return;
    }
    const t = setTimeout(() => setRefreshing(false), REFRESH_TIMEOUT_MS);
    return () => clearTimeout(t);
  }, [rows, refreshing]);

  const handleRefresh = async () => {
    if (refreshing) return;
    refreshStartRef.current = Date.now();
    setRefreshing(true);
    try {
      await invoke("usage_refresh", { provider, accountId: accountId ?? undefined });
    } catch (e) {
      console.warn("usage_refresh failed", e);
      setRefreshing(false);
    }
  };

  const cycle = useMemo(() => activeWeeklyCycle(rows), [rows]);

  const bucketProj = projection?.weekly
    ? {
        avgWeekdaySpend: projection.weekly.avg_weekday_spend,
        avgWeekendSpend: projection.weekly.avg_weekend_spend,
        dowEstimates: projection.weekly.dow_estimates,
        restOfTodayEst: projection.weekly.rest_of_today_est,
        dayActuals: projection.weekly.day_actuals,
      }
    : null;
  const buckets = useMemo(() => {
    const now = Date.now();
    if (range === "week") {
      return buildWeekBuckets(rows, cycle.start, cycle.end, bucketProj);
    }
    if (range === "month") return buildMonthBuckets(rows, now);
    return buildYearBuckets(rows, now);
  }, [rows, range, bucketProj, cycle.start, cycle.end]);

  const rangeLabel = useMemo(() => {
    if (range === "week") {
      return `${formatRangeDate(cycle.start)} → ${formatRangeDate(cycle.end)}`;
    }
    if (range === "month") {
      const d = new Date();
      return d.toLocaleDateString(undefined, { month: "long", year: "numeric" });
    }
    return `${new Date().getFullYear()}`;
  }, [cycle.start, cycle.end, range]);

  const summary = useMemo(() => computeSummary(rows), [rows]);
  const showProjection = range === "week";

  const overlay = (
    <div className="ct-usage ct-usage-modal-overlay" onClick={onClose}>
      <div className="ct-usage-modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h1>{getAgentDescriptor(provider)?.display_name ?? provider} usage</h1>
          <button type="button" className="close-btn" onClick={onClose} aria-label="Close">
            <X size={16} />
          </button>
        </div>

        <section className="ct-usage-card">
          <div className="card-head">
            <div className="card-title">
              <LineChart />
              <span>Usage over time</span>
            </div>
            <div className="head-actions">
              <RangeTabs range={range} onChange={setRange} />
              <button
                type="button"
                className={`refresh-btn ${refreshing ? "spin" : ""}`}
                onClick={handleRefresh}
                disabled={refreshing}
                aria-busy={refreshing}
              >
                <RefreshCw />
                {refreshing ? "Refreshing…" : "Refresh"}
              </button>
            </div>
          </div>

          <div className="ct-usage-summary">
            <Stat label="avg session usage" value={summary.avgSession} state={classify(summary.avgSession)} />
            <Stat label="avg weekly used" value={summary.avgWeekly} state={classify(summary.avgWeekly)} />
            <Stat label="current weekly usage" value={summary.latestWeekly} state={classify(summary.latestWeekly)} />
          </div>

          <UsageChart
            title={range === "week" ? "% per day" : range === "month" ? "weekly peak %" : "monthly peak %"}
            color="var(--u-weekly)"
            buckets={buckets}
            showProjection={showProjection}
            rangeLabel={rangeLabel}
            resetsAt={range === "week" ? cycle.end : null}
          />
        </section>

        <section className="ct-usage-card">
          <div className="card-head">
            <div className="card-title">
              <TableIcon />
              <span>Recent samples</span>
            </div>
            <span style={{ fontFamily: "var(--u-font-mono)", fontSize: 10, color: "var(--ct-muted-fg)", letterSpacing: "0.06em" }}>
              {summary.samples} samples · {summary.errors} parse errors
            </span>
          </div>
          <UsageTable rows={rows} />
          <div className="ct-usage-foot-note">
            last refresh {summary.latestAt ? formatRelative(Date.now(), summary.latestAt) : "—"}
          </div>
        </section>
      </div>
    </div>
  );

  return createPortal(overlay, document.body);
}

function formatRangeDate(ms: number): string {
  return new Date(ms).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

function RangeTabs({ range, onChange }: { range: Range; onChange: (r: Range) => void }) {
  return (
    <div className="ct-usage-panes" role="tablist">
      {(["week", "month", "year"] as Range[]).map((r) => (
        <button
          key={r}
          type="button"
          role="tab"
          aria-selected={range === r}
          className={range === r ? "active" : ""}
          onClick={() => onChange(r)}
        >
          {r}
        </button>
      ))}
    </div>
  );
}

interface StatProps {
  label: string;
  value: number | null;
  state: "neutral" | "warn" | "danger";
  sub?: string;
}
function Stat({ label, value, state, sub }: StatProps) {
  return (
    <div className={`ct-usage-stat ${state}`}>
      <div className="k">{label}</div>
      <div className="v">
        {value == null ? "—" : Math.round(value)}
        <span className="unit">%</span>
      </div>
      {sub && <div className="sub">{sub}</div>}
    </div>
  );
}

interface Summary {
  avgSession: number | null;
  avgWeekly: number | null;
  latestWeekly: number | null;
  latestAt: number | null;
  samples: number;
  errors: number;
}

function computeSummary(rows: UsageSnapshot[]): Summary {
  if (rows.length === 0) {
    return { avgSession: null, avgWeekly: null, latestWeekly: null, latestAt: null, samples: 0, errors: 0 };
  }
  let sessionSum = 0;
  let sessionCount = 0;
  let weeklySum = 0;
  let weeklyCount = 0;
  let errors = 0;
  let latestWeekly: number | null = null;
  let latestAt = 0;
  for (const r of rows) {
    if (!r.parsed_ok) {
      errors += 1;
      continue;
    }
    if (r.session_pct != null) {
      sessionSum += r.session_pct;
      sessionCount += 1;
    }
    if (r.weekly_pct != null) {
      weeklySum += r.weekly_pct;
      weeklyCount += 1;
    }
    if (r.captured_at >= latestAt) {
      latestAt = r.captured_at;
      latestWeekly = r.weekly_pct;
    }
  }
  return {
    avgSession: sessionCount > 0 ? sessionSum / sessionCount : null,
    avgWeekly: weeklyCount > 0 ? weeklySum / weeklyCount : null,
    latestWeekly,
    latestAt: latestAt || null,
    samples: rows.length,
    errors,
  };
}
