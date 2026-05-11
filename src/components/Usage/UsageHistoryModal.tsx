import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { RefreshCw, X, LineChart, Table as TableIcon } from "lucide-react";
import { classify, type UsageSnapshot } from "../../types/usage";
import { useUsageHistory } from "../../hooks/useUsageSnapshot";
import { formatRelative, formatStampShort } from "./format";
import { UsageChart, type Range } from "./UsageChart";
import { UsageTable } from "./UsageTable";

interface UsageHistoryModalProps {
  provider: string;
  accountId: string | null;
  onClose: () => void;
}

const RANGE_MS: Record<Range, number> = {
  "24h": 24 * 60 * 60 * 1000,
  "7d": 7 * 24 * 60 * 60 * 1000,
  "30d": 30 * 24 * 60 * 60 * 1000,
};

export function UsageHistoryModal({ provider, accountId, onClose }: UsageHistoryModalProps) {
  const [range, setRange] = useState<Range>("24h");
  const [refreshToken, setRefreshToken] = useState(0);
  const since = useMemo(() => Date.now() - RANGE_MS[range], [range]);
  const limit = range === "24h" ? 200 : range === "7d" ? 1000 : 5000;
  const { rows, loading } = useUsageHistory({ provider, accountId, sinceMs: since, limit, refetchToken: refreshToken });

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const handleRefresh = async () => {
    try {
      await invoke("usage_refresh", { provider, accountId: accountId ?? undefined });
    } catch (e) {
      console.warn("usage_refresh failed", e);
    }
    setRefreshToken((v) => v + 1);
  };

  const summary = useMemo(() => computeSummary(rows), [rows]);

  const overlay = (
    <div className="ct-usage ct-usage-modal-overlay" onClick={onClose}>
      <div className="ct-usage-modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <div>
            <h1>Claude usage · history</h1>
            <p>
              Token consumption over time, parsed from the Claude CLI status line. Bars and lines reuse the bubble's
              thresholds: neutral &lt; 60%, warning 60–85%, danger &gt; 85%.
            </p>
          </div>
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
              <button type="button" className="refresh-btn" onClick={handleRefresh} disabled={loading}>
                <RefreshCw />
                Refresh
              </button>
            </div>
          </div>

          <div className="ct-usage-summary">
            <Stat label="avg session" value={summary.avgSession} state={classify(summary.avgSession)} />
            <Stat
              label="peak session"
              value={summary.peakSession}
              state={classify(summary.peakSession)}
              sub={summary.peakSessionAt ? `at ${formatStampShort(summary.peakSessionAt)}` : undefined}
            />
            <Stat
              label="weekly used"
              value={summary.latestWeekly}
              state={classify(summary.latestWeekly)}
              sub={summary.samples > 0 ? `${summary.samples} samples · ${summary.errors} errors` : undefined}
            />
          </div>

          <UsageChart title="Session %" color="var(--u-session)" rows={rows} sinceMs={since} field="session_pct" range={range} />
          <UsageChart title="Weekly %" color="var(--u-weekly)" rows={rows} sinceMs={since} field="weekly_pct" range={range} />
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

function RangeTabs({ range, onChange }: { range: Range; onChange: (r: Range) => void }) {
  return (
    <div className="ct-usage-tabs" role="tablist">
      {(["24h", "7d", "30d"] as Range[]).map((r) => (
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
  peakSession: number | null;
  peakSessionAt: number | null;
  latestWeekly: number | null;
  latestAt: number | null;
  samples: number;
  errors: number;
}

function computeSummary(rows: UsageSnapshot[]): Summary {
  if (rows.length === 0) {
    return { avgSession: null, peakSession: null, peakSessionAt: null, latestWeekly: null, latestAt: null, samples: 0, errors: 0 };
  }
  let sum = 0;
  let count = 0;
  let peak: number | null = null;
  let peakAt: number | null = null;
  let errors = 0;
  let latestWeekly: number | null = null;
  let latestAt = 0;
  for (const r of rows) {
    if (!r.parsed_ok) {
      errors += 1;
      continue;
    }
    if (r.session_pct != null) {
      sum += r.session_pct;
      count += 1;
      if (peak == null || r.session_pct > peak) {
        peak = r.session_pct;
        peakAt = r.captured_at;
      }
    }
    if (r.captured_at >= latestAt) {
      latestAt = r.captured_at;
      latestWeekly = r.weekly_pct;
    }
  }
  return {
    avgSession: count > 0 ? sum / count : null,
    peakSession: peak,
    peakSessionAt: peakAt,
    latestWeekly,
    latestAt: latestAt || null,
    samples: rows.length,
    errors,
  };
}
