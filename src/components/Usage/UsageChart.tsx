import { useState } from "react";
import type { Bucket, CycleRange } from "./cycle";
import { BarTooltip, type BarHover } from "./BarTooltip";

export type Range = CycleRange;

const CHART_H = 160;
const PAD_TOP = 8;
const PAD_BOTTOM = 28;
const PAD_LEFT = 28;
const PAD_RIGHT = 8;

interface UsageChartProps {
  title: string;
  color: string;
  buckets: Bucket[];
  showProjection: boolean;
  rangeLabel: string;
  resetsAt?: number | null;
}

export function UsageChart({ title, color, buckets, showProjection, rangeLabel, resetsAt }: UsageChartProps) {
  const [hover, setHover] = useState<BarHover | null>(null);
  const max = Math.max(
    25,
    ...buckets.map((b) => (b.pct == null ? 0 : Math.ceil(b.pct))),
  );
  const niceMax = roundUpNice(max);
  const chartW = 680;
  const innerW = chartW - PAD_LEFT - PAD_RIGHT;
  const innerH = CHART_H - PAD_TOP - PAD_BOTTOM;
  const slot = buckets.length > 0 ? innerW / buckets.length : 0;
  const barW = Math.max(6, slot * 0.55);
  const ticks = [0, niceMax / 4, niceMax / 2, (niceMax / 4) * 3, niceMax];

  return (
    <div className="ct-usage-chart-wrap">
      <div className="ct-usage-chart-head">
        <span className="ct-usage-chart-title">
          <span className="swatch" style={{ background: color }} />
          {title}
        </span>
        <span className="ct-usage-chart-meta">{rangeLabel}</span>
      </div>
      <svg className="ct-usage-chart-svg" viewBox={`0 0 ${chartW} ${CHART_H}`} preserveAspectRatio="none">
        <g stroke="var(--ct-border-subtle)" strokeWidth={1}>
          {ticks.map((t, i) => {
            const y = PAD_TOP + innerH - (t / niceMax) * innerH;
            return <line key={i} x1={PAD_LEFT} y1={y} x2={chartW - PAD_RIGHT} y2={y} opacity={i === 0 ? 1 : 0.4} />;
          })}
        </g>
        <g fontFamily="JetBrains Mono, monospace" fontSize={8.5} fill="var(--ct-muted)">
          {ticks.map((t, i) => {
            const y = PAD_TOP + innerH - (t / niceMax) * innerH;
            return (
              <text key={i} x={PAD_LEFT - 4} y={y + 3} textAnchor="end">
                {Math.round(t)}
              </text>
            );
          })}
        </g>

        {buckets.map((b, i) => {
          const cx = PAD_LEFT + slot * i + slot / 2;
          const px = cx - barW / 2;
          const actual = b.pct == null ? 0 : Math.min(b.pct, niceMax);
          const target = b.target == null ? 0 : Math.min(b.target, niceMax);
          const actualH = (actual / niceMax) * innerH;
          const targetH = (target / niceMax) * innerH;
          const actualY = PAD_TOP + innerH - actualH;
          const targetY = PAD_TOP + innerH - targetH;
          const isFutureOnly = b.projected && showProjection && !b.current;
          const showTarget = showProjection && b.target != null && (b.current || isFutureOnly);
          const dim = b.isFuture && !showProjection;
          const labelVal = isFutureOnly ? null : b.pct;
          // Tooltip anchor: top edge of whichever visible element is highest on screen.
          const tipY = (() => {
            if (isFutureOnly) return targetY;
            if (b.current && showTarget) return Math.min(actualY, targetY);
            if (b.pct != null) return actualY;
            return PAD_TOP + innerH - 8;
          })();
          const onEnter = (e: React.MouseEvent<SVGGElement>) => {
            const g = e.currentTarget as SVGGElement;
            const svg = g.ownerSVGElement;
            if (!svg) return;
            const ctm = svg.getScreenCTM();
            if (!ctm) return;
            const pt = svg.createSVGPoint();
            pt.x = cx;
            pt.y = tipY;
            const scr = pt.matrixTransform(ctm);
            setHover({ bucket: b, x: scr.x, y: scr.y });
          };
          const onLeave = () => setHover(null);
          return (
            <g
              key={i}
              opacity={dim ? 0.25 : 1}
              onMouseEnter={onEnter}
              onMouseLeave={onLeave}
              style={{ cursor: "default" }}
            >
              <rect
                x={px - slot * 0.2}
                y={PAD_TOP}
                width={barW + slot * 0.4}
                height={innerH}
                fill="transparent"
                pointerEvents="all"
              />
              {/* Filled actual bar (past days + today's in-progress fill) */}
              {!isFutureOnly && b.pct != null && (
                <rect
                  x={px}
                  y={actualY}
                  width={barW}
                  height={Math.max(0, actualH)}
                  rx={2}
                  fill={color}
                />
              )}
              {/* Dashed outline = planned daily budget */}
              {showTarget && (
                <rect
                  x={px}
                  y={targetY}
                  width={barW}
                  height={Math.max(0, targetH)}
                  rx={2}
                  fill="transparent"
                  stroke={color}
                  strokeWidth={1.2}
                  strokeDasharray="3 2"
                  opacity={b.current ? 0.7 : 1}
                />
              )}
              <text
                x={cx}
                y={CHART_H - 14}
                textAnchor="middle"
                fontFamily="JetBrains Mono, monospace"
                fontSize={8.5}
                fill="var(--ct-muted)"
              >
                {b.label}
              </text>
              {labelVal != null && labelVal >= 0.5 && (
                <text
                  x={cx}
                  y={actualY - 3}
                  textAnchor="middle"
                  fontFamily="JetBrains Mono, monospace"
                  fontSize={8.5}
                  fill={color}
                  fontWeight={500}
                >
                  {Math.round(labelVal)}
                </text>
              )}
              {isFutureOnly && b.target != null && b.target >= 0.5 && (
                <text
                  x={cx}
                  y={targetY - 3}
                  textAnchor="middle"
                  fontFamily="JetBrains Mono, monospace"
                  fontSize={8.5}
                  fill="var(--ct-muted)"
                  fontWeight={500}
                >
                  {Math.round(b.target)}
                </text>
              )}
            </g>
          );
        })}

        {resetsAt && (
          <g>
            <line
              x1={chartW - PAD_RIGHT}
              y1={PAD_TOP}
              x2={chartW - PAD_RIGHT}
              y2={PAD_TOP + innerH}
              stroke="var(--u-warn)"
              strokeWidth={1}
              strokeDasharray="2 3"
              opacity={0.6}
            />
            <text
              x={chartW - PAD_RIGHT - 4}
              y={PAD_TOP + 10}
              textAnchor="end"
              fontFamily="JetBrains Mono, monospace"
              fontSize={8.5}
              fill="var(--u-warn)"
              opacity={0.85}
            >
              resets {formatResetMoment(resetsAt)}
            </text>
          </g>
        )}
      </svg>
      {hover && <BarTooltip hover={hover} showProjection={showProjection} />}
    </div>
  );
}

function formatResetMoment(ms: number): string {
  const d = new Date(ms);
  const date = d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  const time = d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", hour12: false });
  return `${date} ${time}`;
}

function roundUpNice(v: number): number {
  if (v <= 25) return 25;
  if (v <= 50) return 50;
  if (v <= 75) return 75;
  return 100;
}
