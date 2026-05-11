import { useMemo, useState } from "react";
import { RefreshCw, ArrowRight } from "lucide-react";
import { classify, type UsageSnapshot } from "../../types/usage";
import { useUsageHistory } from "../../hooks/useUsageSnapshot";
import { formatRelative, formatResetsAt } from "./format";

interface UsageDetailProps {
  snapshot: UsageSnapshot;
  flipUp: boolean;
  onRefresh: () => Promise<void>;
  onOpenHistory: () => void;
}

const SPARK_W = 280;
const SPARK_H = 48;
const TWENTY_FOUR_H_MS = 24 * 60 * 60 * 1000;

export function UsageDetail({ snapshot, flipUp, onRefresh, onOpenHistory }: UsageDetailProps) {
  const [refreshing, setRefreshing] = useState(false);
  const [series, setSeries] = useState<"session" | "weekly">("session");
  const since = useMemo(() => Date.now() - TWENTY_FOUR_H_MS, []);
  const { rows } = useUsageHistory({ provider: snapshot.provider, accountId: snapshot.account_id, sinceMs: since });

  const sessionState = classify(snapshot.session_pct);
  const weeklyState = classify(snapshot.weekly_pct);
  const now = Date.now();

  const handleRefresh = async () => {
    setRefreshing(true);
    try {
      await onRefresh();
    } finally {
      setRefreshing(false);
    }
  };

  return (
    <div className={`ct-usage-popover ${flipUp ? "up" : "down"}`} role="dialog" aria-label="usage details">
      <div className="head">
        <span className="title">claude usage</span>
        <button
          type="button"
          className={`ico-btn ${refreshing ? "spin" : ""}`}
          onClick={handleRefresh}
          title="Refresh"
          aria-label="Refresh usage"
          disabled={refreshing}
        >
          <RefreshCw />
        </button>
      </div>

      <Gauge
        label="Session"
        pct={snapshot.session_pct}
        resetsAt={snapshot.session_resets_at}
        state={sessionState}
        now={now}
      />
      <Gauge
        label="Weekly"
        pct={snapshot.weekly_pct}
        resetsAt={snapshot.weekly_resets_at}
        state={weeklyState}
        now={now}
      />

      <Sparkline rows={rows} series={series} onToggle={setSeries} sinceMs={since} />

      <div className="ct-usage-foot">
        <span className="live">
          <span className="pulse" />
          <span>last updated {formatRelative(now, snapshot.captured_at)}</span>
        </span>
        <button type="button" className="link" onClick={onOpenHistory}>
          <span>History</span>
          <ArrowRight size={11} />
        </button>
      </div>
    </div>
  );
}

interface GaugeProps {
  label: string;
  pct: number | null;
  resetsAt: number | null;
  state: "neutral" | "warn" | "danger";
  now: number;
}

function Gauge({ label, pct, resetsAt, state, now }: GaugeProps) {
  const width = pct == null ? 0 : Math.max(0, Math.min(100, pct));
  return (
    <div className={`ct-usage-gauge ${state}`}>
      <span className="label">{label}</span>
      <span className="pct">{pct == null ? "—" : `${Math.round(pct)}%`}</span>
      <div className="bar">
        <div className="fill" style={{ width: `${width}%` }} />
      </div>
      <span className="reset">{resetsAt ? formatResetsAt(resetsAt, now) : "—"}</span>
    </div>
  );
}

interface SparklineProps {
  rows: UsageSnapshot[];
  series: "session" | "weekly";
  onToggle: (s: "session" | "weekly") => void;
  sinceMs: number;
}

interface SparkPoint {
  x: number;
  y: number;
  ok: boolean;
}

function Sparkline({ rows, series, onToggle, sinceMs }: SparklineProps) {
  const now = Date.now();
  const span = Math.max(1, now - sinceMs);

  const points: SparkPoint[] = useMemo(() => {
    const out: SparkPoint[] = [];
    for (const r of rows) {
      const t = r.captured_at;
      if (t < sinceMs) continue;
      const v = series === "session" ? r.session_pct : r.weekly_pct;
      const x = ((t - sinceMs) / span) * SPARK_W;
      const ok = r.parsed_ok && v != null;
      const y = ok ? SPARK_H - 6 - (Math.max(0, Math.min(100, v!)) / 100) * (SPARK_H - 10) : SPARK_H - 6;
      out.push({ x, y, ok });
    }
    return out;
  }, [rows, series, sinceMs, span, now]);

  const segments = useMemo(() => splitSegments(points), [points]);
  const last = points[points.length - 1];
  const stroke = series === "session" ? "var(--u-neutral)" : "var(--u-weekly)";

  return (
    <div className="ct-usage-spark">
      <div className="head">
        <span>last 24h</span>
        <span className="ct-usage-tabs" style={{ padding: 1 }}>
          <button
            type="button"
            className={series === "session" ? "active" : ""}
            onClick={() => onToggle("session")}
          >
            session
          </button>
          <button
            type="button"
            className={series === "weekly" ? "active" : ""}
            onClick={() => onToggle("weekly")}
          >
            weekly
          </button>
        </span>
      </div>
      <svg viewBox={`0 0 ${SPARK_W} ${SPARK_H}`} preserveAspectRatio="none" aria-label={`${series} % last 24h`}>
        <line x1={0} y1={SPARK_H - 6} x2={SPARK_W} y2={SPARK_H - 6} stroke="var(--ct-border-subtle)" strokeWidth={1} />
        {segments.map((seg, i) => {
          if (seg.kind === "ok") {
            return (
              <polyline
                key={i}
                points={seg.pts.map((p) => `${p.x.toFixed(1)},${p.y.toFixed(1)}`).join(" ")}
                fill="none"
                stroke={stroke}
                strokeWidth={1.4}
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            );
          }
          return (
            <g key={i}>
              <line
                x1={seg.from.x}
                y1={seg.from.y}
                x2={seg.to.x}
                y2={seg.to.y}
                stroke="var(--ct-muted)"
                strokeWidth={1.2}
                strokeDasharray="2 3"
                opacity={0.7}
              />
              <circle cx={(seg.from.x + seg.to.x) / 2} cy={(seg.from.y + seg.to.y) / 2} r={6} fill="var(--u-bg-elev)" stroke="var(--u-warn)" strokeWidth={1} />
              <text x={(seg.from.x + seg.to.x) / 2} y={(seg.from.y + seg.to.y) / 2 + 3} textAnchor="middle" fontFamily="JetBrains Mono, monospace" fontSize={8} fill="var(--u-warn)" fontWeight={600}>?</text>
            </g>
          );
        })}
        {last && last.ok && (
          <circle cx={last.x} cy={last.y} r={2.4} fill="var(--u-bg-elev)" stroke={stroke} strokeWidth={1.6} />
        )}
      </svg>
    </div>
  );
}

type Segment =
  | { kind: "ok"; pts: SparkPoint[] }
  | { kind: "gap"; from: SparkPoint; to: SparkPoint };

function splitSegments(points: SparkPoint[]): Segment[] {
  const segs: Segment[] = [];
  let run: SparkPoint[] = [];
  for (let i = 0; i < points.length; i++) {
    const p = points[i];
    if (p.ok) {
      run.push(p);
    } else {
      if (run.length > 0) segs.push({ kind: "ok", pts: run });
      const prev = points[i - 1];
      const next = points[i + 1];
      if (prev && prev.ok && next && next.ok) {
        segs.push({ kind: "gap", from: prev, to: next });
      }
      run = [];
    }
  }
  if (run.length > 0) segs.push({ kind: "ok", pts: run });
  return segs;
}
