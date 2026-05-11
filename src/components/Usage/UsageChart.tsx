import { useMemo } from "react";
import type { UsageSnapshot } from "../../types/usage";

export type Range = "24h" | "7d" | "30d";

const CHART_W = 680;
const CHART_H = 140;

interface UsageChartProps {
  title: string;
  color: string;
  rows: UsageSnapshot[];
  sinceMs: number;
  field: "session_pct" | "weekly_pct";
  range: Range;
}

interface ChartPoint {
  x: number;
  y: number;
  ok: boolean;
}

type Segment =
  | { kind: "ok"; pts: ChartPoint[] }
  | { kind: "gap"; from: ChartPoint; to: ChartPoint };

export function UsageChart({ title, color, rows, sinceMs, field, range }: UsageChartProps) {
  const now = Date.now();
  const span = Math.max(1, now - sinceMs);

  const points = useMemo(() => {
    const out: ChartPoint[] = [];
    for (const r of rows) {
      if (r.captured_at < sinceMs) continue;
      const v = r[field];
      const x = ((r.captured_at - sinceMs) / span) * CHART_W;
      const ok = r.parsed_ok && v != null;
      const y = ok ? 2 + (1 - Math.max(0, Math.min(100, v!)) / 100) * (CHART_H - 5) : CHART_H - 5;
      out.push({ x, y, ok });
    }
    return out;
  }, [rows, field, sinceMs, span]);

  const segments = useMemo(() => splitSegments(points), [points]);
  const last = points[points.length - 1];
  const tickLabels = rangeTickLabels(range);

  return (
    <div className="ct-usage-chart-wrap">
      <div className="ct-usage-chart-head">
        <span className="ct-usage-chart-title">
          <span className="swatch" style={{ background: color }} />
          {title}
        </span>
        <span className="ct-usage-chart-meta">{rangeMeta(range, sinceMs)}</span>
      </div>
      <svg className="ct-usage-chart-svg" viewBox={`0 0 ${CHART_W} ${CHART_H}`} preserveAspectRatio="none">
        <g stroke="var(--ct-border-subtle)" strokeWidth={1}>
          <line x1={0} y1={2} x2={CHART_W} y2={2} />
          <line x1={0} y1={34} x2={CHART_W} y2={34} />
          <line x1={0} y1={68} x2={CHART_W} y2={68} />
          <line x1={0} y1={102} x2={CHART_W} y2={102} />
          <line x1={0} y1={135} x2={CHART_W} y2={135} />
        </g>
        <g fontFamily="JetBrains Mono, monospace" fontSize={8.5} fill="var(--ct-muted)">
          <text x={2} y={6}>100</text>
          <text x={2} y={38}>75</text>
          <text x={2} y={72}>50</text>
          <text x={2} y={106}>25</text>
          <text x={2} y={139}>0</text>
        </g>
        <line x1={0} y1={54} x2={CHART_W} y2={54} stroke="var(--u-warn)" strokeWidth={0.8} strokeDasharray="3 5" opacity={0.45} />
        <line x1={0} y1={22} x2={CHART_W} y2={22} stroke="var(--u-danger)" strokeWidth={0.8} strokeDasharray="3 5" opacity={0.45} />

        {segments.map((seg, i) => {
          if (seg.kind === "ok") {
            return (
              <polyline
                key={i}
                points={seg.pts.map((p) => `${p.x.toFixed(1)},${p.y.toFixed(1)}`).join(" ")}
                fill="none"
                stroke={color}
                strokeWidth={1.6}
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            );
          }
          const mx = (seg.from.x + seg.to.x) / 2;
          const my = (seg.from.y + seg.to.y) / 2;
          return (
            <g key={i}>
              <line x1={seg.from.x} y1={seg.from.y} x2={seg.to.x} y2={seg.to.y} stroke="var(--ct-muted)" strokeWidth={1.4} strokeDasharray="3 4" opacity={0.7} />
              <circle cx={mx} cy={my} r={8} fill="var(--u-bg-elev)" stroke="var(--u-warn)" strokeWidth={1.2} />
              <text x={mx} y={my + 3} textAnchor="middle" fontFamily="JetBrains Mono, monospace" fontSize={9.5} fill="var(--u-warn)" fontWeight={600}>?</text>
            </g>
          );
        })}

        {last && last.ok && (
          <circle cx={last.x} cy={last.y} r={3.2} fill="var(--u-bg-elev)" stroke={color} strokeWidth={1.8} />
        )}

        <g fontFamily="JetBrains Mono, monospace" fontSize={8.5} fill="var(--ct-muted)">
          {tickLabels.map((t, i) => (
            <text
              key={i}
              x={i === tickLabels.length - 1 ? CHART_W - 24 : (i / (tickLabels.length - 1)) * CHART_W}
              y={138}
            >
              {t}
            </text>
          ))}
        </g>
      </svg>
    </div>
  );
}

function splitSegments(points: ChartPoint[]): Segment[] {
  const segs: Segment[] = [];
  let run: ChartPoint[] = [];
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

function rangeTickLabels(range: Range): string[] {
  if (range === "24h") return ["00", "06", "12", "18", "now"];
  if (range === "7d") return ["-7d", "-5d", "-3d", "-1d", "now"];
  return ["-30d", "-22d", "-15d", "-7d", "now"];
}

function rangeMeta(range: Range, since: number): string {
  if (range === "24h") {
    const d = new Date(since);
    return `${d.toLocaleDateString(undefined, { month: "short", day: "numeric" })} → now (24h)`;
  }
  if (range === "7d") return "rolling 7 days → now";
  return "rolling 30 days → now";
}
