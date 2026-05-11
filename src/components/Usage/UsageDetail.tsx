import { useEffect, useMemo, useRef, useState } from "react";
import { RefreshCw, ArrowRight } from "lucide-react";
import { classify, type UsageSnapshot } from "../../types/usage";
import { useUsageHistory } from "../../hooks/useUsageSnapshot";
import { formatRelative, formatResetsAt } from "./format";
import { activeWeeklyCycle, buildWeekBuckets, type Bucket } from "./cycle";
import { BarTooltip, type BarHover } from "./BarTooltip";

interface UsageDetailProps {
  snapshot: UsageSnapshot;
  flipUp: boolean;
  onRefresh: () => Promise<void>;
  onOpenHistory: () => void;
}

const SPARK_W = 280;
const SPARK_H = 78;
const EIGHT_DAYS_MS = 8 * 24 * 60 * 60 * 1000;
const REFRESH_TIMEOUT_MS = 12_000;

export function UsageDetail({ snapshot, flipUp, onRefresh, onOpenHistory }: UsageDetailProps) {
  const [refreshing, setRefreshing] = useState(false);
  const refreshStartRef = useRef<number>(0);
  const since = useMemo(() => Date.now() - EIGHT_DAYS_MS, []);
  const { rows } = useUsageHistory({
    provider: snapshot.provider,
    accountId: snapshot.account_id,
    sinceMs: since,
  });

  const sessionState = classify(snapshot.session_pct);
  const weeklyState = classify(snapshot.weekly_pct);
  const now = Date.now();

  useEffect(() => {
    if (!refreshing) return;
    if (snapshot.captured_at > refreshStartRef.current) {
      setRefreshing(false);
      return;
    }
    const t = setTimeout(() => setRefreshing(false), REFRESH_TIMEOUT_MS);
    return () => clearTimeout(t);
  }, [snapshot.captured_at, refreshing]);

  const handleRefresh = async () => {
    if (refreshing) return;
    refreshStartRef.current = Date.now();
    setRefreshing(true);
    try {
      await onRefresh();
    } catch {
      setRefreshing(false);
    }
  };

  const buckets = useMemo(() => {
    const cycle = activeWeeklyCycle(rows);
    return buildWeekBuckets(rows, cycle.start, cycle.end, true);
  }, [rows]);

  const sessionProj = useMemo(
    () =>
      projectAtReset(
        rows,
        "session_pct",
        "session_resets_at",
        snapshot.session_resets_at,
        snapshot.session_pct,
        now,
      ),
    [rows, snapshot.session_pct, snapshot.session_resets_at, now],
  );
  const weeklyProj = useMemo(
    () =>
      projectAtReset(
        rows,
        "weekly_pct",
        "weekly_resets_at",
        snapshot.weekly_resets_at,
        snapshot.weekly_pct,
        now,
      ),
    [rows, snapshot.weekly_pct, snapshot.weekly_resets_at, now],
  );

  return (
    <div className={`ct-usage-popover ${flipUp ? "up" : "down"}`} role="dialog" aria-label="usage details">
      <div className="head">
        <span className="title">{snapshot.provider} usage</span>
        <button
          type="button"
          className={`ico-btn ${refreshing ? "spin" : ""}`}
          onClick={handleRefresh}
          title={refreshing ? "Refreshing…" : "Refresh"}
          aria-label="Refresh usage"
          aria-busy={refreshing}
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
        projection={sessionProj}
        now={now}
      />
      <Gauge
        label="Weekly"
        pct={snapshot.weekly_pct}
        resetsAt={snapshot.weekly_resets_at}
        state={weeklyState}
        projection={weeklyProj}
        now={now}
      />

      <MiniWeekBars buckets={buckets} />

      <div className="ct-usage-foot">
        <span className="live">
          <span className="pulse" />
          <span>last updated {formatRelative(now, snapshot.captured_at)}</span>
        </span>
        <button type="button" className="link" onClick={onOpenHistory}>
          <span>Full view</span>
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
  projection: Projection | null;
  now: number;
}

function Gauge({ label, pct, resetsAt, state, projection, now }: GaugeProps) {
  const width = pct == null ? 0 : Math.max(0, Math.min(100, pct));
  const projAt = projection ? Math.min(100, Math.max(0, projection.value)) : null;
  const projState: "warn" | "danger" | "neutral" = projection
    ? projection.exceeded
      ? "danger"
      : projection.value >= 60
        ? "warn"
        : "neutral"
    : "neutral";
  return (
    <div className={`ct-usage-gauge ${state}`}>
      <span className="label">{label}</span>
      <span className="pct">{pct == null ? "—" : `${Math.round(pct)}%`}</span>
      <div className="bar">
        <div className="fill" style={{ width: `${width}%` }} />
        {projAt != null && projection && (
          <div
            className={`projmark ${projState}`}
            style={{ left: `${projAt}%` }}
            title={
              projection.exceeded
                ? `Projected to hit 100% before reset · ${Math.round(projection.value)}% at this rate`
                : `Projected ${Math.round(projection.value)}% by reset`
            }
          />
        )}
      </div>
      <span className="reset">
        {resetsAt ? formatResetsAt(resetsAt, now) : "—"}
        {projection && (
          <span className={`proj-note ${projState}`}>
            {" · "}
            {projection.exceeded
              ? `→ over 100% at this rate`
              : `→ ${Math.round(projection.value)}% projected`}
          </span>
        )}
      </span>
    </div>
  );
}

interface Projection {
  value: number;
  exceeded: boolean;
}

function projectAtReset(
  rows: UsageSnapshot[],
  field: "session_pct" | "weekly_pct",
  resetField: "session_resets_at" | "weekly_resets_at",
  currentResetAt: number | null,
  currentPct: number | null,
  now: number,
): Projection | null {
  if (currentResetAt == null || currentPct == null) return null;
  const sameWindow = rows
    .filter(
      (r) =>
        r.parsed_ok &&
        r[field] != null &&
        r[resetField] === currentResetAt &&
        r.captured_at <= now,
    )
    .sort((a, b) => a.captured_at - b.captured_at);
  if (sameWindow.length < 2) return null;
  const first = sameWindow[0];
  const last = sameWindow[sameWindow.length - 1];
  const dt = last.captured_at - first.captured_at;
  if (dt < 60_000) return null;
  const dPct = (last[field] ?? 0) - (first[field] ?? 0);
  const rate = dPct / dt;
  if (rate <= 0) return null;
  const projected = currentPct + rate * Math.max(0, currentResetAt - now);
  return {
    value: Math.max(0, Math.min(200, projected)),
    exceeded: projected >= 100,
  };
}

function MiniWeekBars({ buckets }: { buckets: Bucket[] }) {
  const [hover, setHover] = useState<BarHover | null>(null);
  const niceMax = Math.max(
    25,
    ...buckets.map((b) => (b.pct == null ? 0 : Math.ceil(b.pct))),
  );
  const ceiling = niceMax <= 25 ? 25 : niceMax <= 50 ? 50 : niceMax <= 75 ? 75 : 100;
  const slot = buckets.length > 0 ? SPARK_W / buckets.length : 0;
  const barW = Math.max(8, slot * 0.6);
  const VALUE_BAND = 11;
  const LABEL_BAND = 11;
  const innerH = SPARK_H - VALUE_BAND - LABEL_BAND;
  const baseY = VALUE_BAND + innerH;

  return (
    <div className="ct-usage-spark">
      <svg viewBox={`0 0 ${SPARK_W} ${SPARK_H}`} preserveAspectRatio="none" aria-label="weekly usage per day">
        <line x1={0} y1={baseY} x2={SPARK_W} y2={baseY} stroke="var(--ct-border-subtle)" strokeWidth={1} />
        {buckets.map((b, i) => {
          const cx = slot * i + slot / 2;
          const px = cx - barW / 2;
          const actual = b.pct == null ? 0 : Math.min(b.pct, ceiling);
          const target = b.target == null ? 0 : Math.min(b.target, ceiling);
          const actualH = (actual / ceiling) * innerH;
          const targetH = (target / ceiling) * innerH;
          const actualY = baseY - actualH;
          const targetY = baseY - targetH;
          const isFutureOnly = b.projected && !b.current;
          const showTarget = b.target != null && (b.current || isFutureOnly);
          const dim = b.isFuture && !b.projected;
          const color = "var(--u-weekly)";
          const labelVal = isFutureOnly ? b.target : b.pct;
          const tipY = (() => {
            if (isFutureOnly) return targetY;
            if (b.current && showTarget) return Math.min(actualY, targetY);
            if (b.pct != null) return actualY;
            return baseY - 4;
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
              opacity={dim ? 0.3 : 1}
              onMouseEnter={onEnter}
              onMouseLeave={onLeave}
              style={{ cursor: "default" }}
            >
              <rect
                x={px - slot * 0.2}
                y={0}
                width={barW + slot * 0.4}
                height={baseY}
                fill="transparent"
                pointerEvents="all"
              />
              {!isFutureOnly && b.pct != null && (
                <rect
                  x={px}
                  y={actualY}
                  width={barW}
                  height={Math.max(0, actualH)}
                  rx={1.5}
                  fill={color}
                />
              )}
              {showTarget && (
                <rect
                  x={px}
                  y={targetY}
                  width={barW}
                  height={Math.max(0, targetH)}
                  rx={1.5}
                  fill="transparent"
                  stroke={color}
                  strokeWidth={1}
                  strokeDasharray="2 2"
                  opacity={b.current ? 0.7 : 1}
                />
              )}
              {labelVal != null && labelVal >= 0.5 && (
                <text
                  x={cx}
                  y={(isFutureOnly ? targetY : actualY) - 2}
                  textAnchor="middle"
                  fontFamily="JetBrains Mono, monospace"
                  fontSize={8}
                  fill={isFutureOnly ? "var(--ct-muted)" : color}
                  fontWeight={500}
                >
                  {Math.round(labelVal)}
                </text>
              )}
              <text
                x={cx}
                y={SPARK_H - 2}
                textAnchor="middle"
                fontFamily="JetBrains Mono, monospace"
                fontSize={8}
                fill="var(--ct-muted)"
              >
                {b.label[0]}
              </text>
            </g>
          );
        })}
      </svg>
      {hover && <BarTooltip hover={hover} showProjection />}
    </div>
  );
}
