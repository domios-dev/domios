import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { RefreshCw, ArrowRight } from "lucide-react";
import { classify, type UsageSnapshot } from "../../types/usage";
import {
  useUsageHistory,
  useUsageProjection,
  type SessionProjection,
  type WeeklyProjection,
} from "../../hooks/useUsageSnapshot";
import { formatRelative, formatResetsAt } from "./format";
import {
  activeWeeklyCycle,
  buildWeekBuckets,
  cumByBucket,
  findOverspendIndex,
  type Bucket,
} from "./cycle";
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
const STALE_MS = 20 * 60 * 1000;
const REFRESH_TIMEOUT_MS = 12_000;
const MERGE_MARKERS_WITHIN_PCT = 3;
const INTRO_HINT_OPENS_KEY = "ct-usage-intro-hint-opens";
const INTRO_HINT_MAX_OPENS = 5;

export function UsageDetail({ snapshot, flipUp, onRefresh, onOpenHistory }: UsageDetailProps) {
  const [refreshing, setRefreshing] = useState(false);
  const refreshStartRef = useRef<number>(0);
  const showIntroHints = useMemo(() => {
    try {
      const n = parseInt(localStorage.getItem(INTRO_HINT_OPENS_KEY) || "0", 10) || 0;
      localStorage.setItem(INTRO_HINT_OPENS_KEY, String(n + 1));
      return n < INTRO_HINT_MAX_OPENS;
    } catch {
      return true;
    }
  }, []);
  const since = useMemo(() => Date.now() - EIGHT_DAYS_MS, []);
  const { rows } = useUsageHistory({
    provider: snapshot.provider,
    accountId: snapshot.account_id,
    sinceMs: since,
  });

  const sessionState = classify(snapshot.session_pct);
  const weeklyState = classify(snapshot.weekly_pct);
  const now = Date.now();
  const isStale = now - snapshot.captured_at > STALE_MS;

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

  const cycle = useMemo(() => activeWeeklyCycle(rows), [rows]);
  const { projection } = useUsageProjection({
    provider: snapshot.provider,
    accountId: snapshot.account_id,
  });
  const bucketProj = projection?.weekly
    ? {
        avgWeekdaySpend: projection.weekly.avg_weekday_spend,
        avgWeekendSpend: projection.weekly.avg_weekend_spend,
        dowEstimates: projection.weekly.dow_estimates,
        restOfTodayEst: projection.weekly.rest_of_today_est,
        dayActuals: projection.weekly.day_actuals,
      }
    : null;
  const buckets = useMemo(
    () => buildWeekBuckets(rows, cycle.start, cycle.end, bucketProj),
    [rows, cycle.start, cycle.end, bucketProj],
  );

  const sessionMarkers = useMemo(
    () => sessionProjectionMarkers(projection?.session ?? null),
    [projection?.session],
  );
  const weeklyMarkers = useMemo(
    () => weeklyProjectionMarkers(projection?.weekly ?? null, snapshot.weekly_pct),
    [projection?.weekly, snapshot.weekly_pct],
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

      {isStale && (
        <div className="ct-usage-stale-banner">
          Data may be outdated — last updated {formatRelative(now, snapshot.captured_at)}
        </div>
      )}

      <Gauge
        label="Session"
        variant="session"
        pct={snapshot.session_pct}
        resetsAt={snapshot.session_resets_at}
        state={sessionState}
        markers={sessionMarkers}
        now={now}
        showIntroHint={showIntroHints}
      />
      <Gauge
        label="Weekly"
        variant="weekly"
        pct={snapshot.weekly_pct}
        resetsAt={snapshot.weekly_resets_at}
        state={weeklyState}
        markers={weeklyMarkers}
        now={now}
        showIntroHint={showIntroHints}
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

function shortDesc(title: string): string {
  switch (title) {
    case "End of today":
      return "by end of day";
    case "End of cycle":
      return "by end of cycle";
    case "Projection at reset":
      return "by session reset";
    default:
      return "";
  }
}

// ---------- Gauge with surveyor-mark projection markers ----------

type MarkerVariant = "session" | "weekly" | "success" | "warn" | "danger";

interface GaugeMarker {
  posPct: number; // pinned to right edge if > 100
  valuePct: number;
  variant: MarkerVariant;
  title: string;
  desc: string;
}

interface GaugeProps {
  label: string;
  variant: "session" | "weekly";
  pct: number | null;
  resetsAt: number | null;
  state: "neutral" | "warn" | "danger";
  markers: GaugeMarker[];
  now: number;
  showIntroHint: boolean;
}

function Gauge({ label, variant, pct, resetsAt, state, markers, now, showIntroHint }: GaugeProps) {
  const width = pct == null ? 0 : Math.max(0, Math.min(100, pct));
  return (
    <div className={`ct-usage-gauge ${variant} ${state}`}>
      <span className="label">{label}</span>
      <span className="pct">{pct == null ? "—" : `${Math.round(pct)}%`}</span>
      <div className="bar-wrap">
        <div className="bar">
          <div className="fill" style={{ width: `${width}%` }} />
        </div>
        {(() => {
          const pinnedIdx = markers.findIndex((m) => m.posPct > 100);
          const hintIdx = pinnedIdx >= 0 ? pinnedIdx : 0;
          const showLabel = labelVisibility(markers);
          return markers.map((m, i) => (
            <Marker
              key={i}
              marker={m}
              variantDefault={variant}
              introHint={showIntroHint && i === hintIdx ? introFor(m) : undefined}
              showLabel={showLabel[i]}
            />
          ));
        })()}
      </div>
      <span className="reset">{resetsAt ? formatResetsAt(resetsAt, now) : "—"}</span>
    </div>
  );
}

function introFor(m: GaugeMarker): string {
  if (m.posPct > 100) {
    return "at this pace, will overspend by cycle end";
  }
  return `projected ${Math.round(m.valuePct)}% ${shortDesc(m.title)}`;
}

// Hide a marker's % label if it would visually collide with an earlier marker.
// "Earlier" = lower posPct (rendered to the left). Threshold ≈ 9% of bar width.
function labelVisibility(markers: GaugeMarker[]): boolean[] {
  const MIN_SPACING_PCT = 9;
  const result: boolean[] = markers.map(() => true);
  const sorted = markers
    .map((m, i) => ({ m, i, x: Math.min(100, m.posPct) }))
    .sort((a, b) => a.x - b.x);
  let lastShown = -Infinity;
  for (const item of sorted) {
    if (item.x - lastShown < MIN_SPACING_PCT) {
      result[item.i] = false;
    } else {
      lastShown = item.x;
    }
  }
  return result;
}

interface MarkerProps {
  marker: GaugeMarker;
  variantDefault: "session" | "weekly";
  introHint?: string;
  showLabel?: boolean;
}

function Marker({ marker, variantDefault, introHint, showLabel = true }: MarkerProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [tipPos, setTipPos] = useState<{ x: number; y: number } | null>(null);
  const [introVisible, setIntroVisible] = useState(Boolean(introHint));
  const isPinned = marker.posPct > 100;
  const left = Math.max(0, Math.min(100, marker.posPct));
  const variant = marker.variant === "session" || marker.variant === "weekly" ? variantDefault : marker.variant;

  useEffect(() => {
    if (!introHint) return;
    const t = setTimeout(() => setIntroVisible(false), 4500);
    return () => clearTimeout(t);
  }, [introHint]);

  const show = () => {
    setIntroVisible(false);
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const rawX = r.left + r.width / 2;
    const TIP_HALF_W = 120;
    const MARGIN = 8;
    const x = Math.max(
      MARGIN + TIP_HALF_W,
      Math.min(window.innerWidth - MARGIN - TIP_HALF_W, rawX),
    );
    setTipPos({ x, y: r.top });
  };
  const hide = () => setTipPos(null);

  return (
    <div
      ref={ref}
      className={`ct-usage-mark ${variant} ${isPinned ? "pinned" : ""}`}
      style={{ left: `${left}%` }}
      onMouseEnter={show}
      onMouseLeave={hide}
      onFocus={show}
      onBlur={hide}
      tabIndex={0}
    >
      <span className="dot" />
      {showLabel && !introVisible && !tipPos && marker.posPct <= 100 && (
        <span className="num" aria-hidden>
          {Math.round(marker.valuePct)}%
        </span>
      )}
      {introVisible && introHint && !tipPos && (
        <span className="intro-hint" aria-hidden>
          {introHint}
        </span>
      )}
      {tipPos &&
        createPortal(
          <div className={`ct-usage-mark-tip ${variant} ${isPinned ? "pinned" : ""}`} style={{ left: tipPos.x, top: tipPos.y }} role="tooltip">
            <div className="t-title">{marker.title}</div>
            {!isPinned && (
              <div className="t-num">
                {Math.round(marker.valuePct)}
                <span className="unit">%</span>
              </div>
            )}
            <div className="t-desc">{marker.desc}</div>
          </div>,
          document.body,
        )}
    </div>
  );
}

// ---------- Projection → marker conversion ----------

function sessionProjectionMarkers(proj: SessionProjection | null): GaugeMarker[] {
  if (!proj) return [];
  const v = proj.projected_pct;
  return [
    {
      posPct: v,
      valuePct: v,
      variant: v >= 100 ? "danger" : v >= 80 ? "warn" : "session",
      title: "Projection at reset",
      desc: `If you keep this pace, session usage hits ${Math.round(v)}% before the 5h window resets.`,
    },
  ];
}

function weeklyProjectionMarkers(
  proj: WeeklyProjection | null,
  currentWeeklyPct: number | null,
): GaugeMarker[] {
  if (!proj || currentWeeklyPct == null) return [];
  const eod = proj.end_of_today_pct;
  const eoc = proj.end_of_cycle_pct;
  const eodExceeded = eod > 100;
  const eocExceeded = eoc > 100;
  const merge = Math.abs(eod - eoc) <= MERGE_MARKERS_WITHIN_PCT;

  const markers: GaugeMarker[] = [];

  if (!merge) {
    markers.push({
      posPct: eod,
      valuePct: eod,
      variant: eodExceeded ? "danger" : eod >= 80 ? "warn" : "weekly",
      title: "End of today",
      desc: `Projected weekly usage by midnight today based on your historical pace.`,
    });
  }

  markers.push({
    posPct: eoc,
    valuePct: eoc,
    variant: eocExceeded ? "danger" : eoc >= 80 ? "warn" : "success",
    title: "End of cycle",
    desc: eocExceeded
      ? `On current trend, you'll exceed the weekly limit by the time the cycle resets.`
      : `On current trend you'll finish the week at ${Math.round(eoc)}%. ${proj.confidence === "low" ? "Low confidence — not enough history." : ""}`,
  });

  return markers;
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
  const overspendIdx = findOverspendIndex(buckets);
  const cum = cumByBucket(buckets);

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
            setHover({
              bucket: b,
              x: scr.x,
              y: scr.y,
              cumEnd: cum[i],
              isOverspendStart: i === overspendIdx,
              isAfterOverspend: overspendIdx >= 0 && i > overspendIdx,
            });
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
                  stroke={overspendIdx >= 0 && i >= overspendIdx ? "var(--u-danger)" : color}
                  strokeWidth={overspendIdx >= 0 && i >= overspendIdx ? 1.3 : 1}
                  strokeDasharray="2 2"
                  opacity={b.current ? 0.7 : 1}
                />
              )}
              {i === overspendIdx && (
                <line
                  x1={cx}
                  y1={VALUE_BAND}
                  x2={cx}
                  y2={baseY}
                  stroke="var(--u-danger)"
                  strokeWidth={1}
                  strokeDasharray="2 2"
                  opacity={0.7}
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
