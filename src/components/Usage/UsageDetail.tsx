import { useEffect, useMemo, useRef, useState } from "react";
import { RefreshCw, ArrowRight } from "lucide-react";
import { classify, type UsageSnapshot } from "../../types/usage";
import { getCachedConfig, setUiSettings } from "../../lib/clientSettings";
import {
  useUsageHistory,
  useUsageProjection,
  type SessionProjection,
  type WeeklyProjection,
} from "../../hooks/useUsageSnapshot";
import { formatRelative, formatResetsAt, formatSpend } from "./format";
import {
  activeWeeklyCycle,
  buildWeekBuckets,
  cumByBucket,
  findOverspendIndex,
  type Bucket,
} from "./cycle";
import { BarTooltip, type BarHover } from "./BarTooltip";
import { UsageGauge, type GaugeMarker } from "./UsageGauge";
import { visibleUsageMetrics } from "./metrics";
import { staleBannerCopy } from "./staleBanner";

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
const INTRO_HINT_MAX_OPENS = 5;

function StaleBanner({ snapshot, now }: { snapshot: UsageSnapshot; now: number }) {
  const copy = staleBannerCopy(snapshot);
  return (
    <div className="ct-usage-stale-banner" data-usage-stale-cause={copy.cause}>
      {copy.headline} — last updated {formatRelative(now, snapshot.captured_at)}
      {copy.detail && <div className="ct-usage-stale-detail">{copy.detail}</div>}
    </div>
  );
}

export function UsageDetail({ snapshot, flipUp, onRefresh, onOpenHistory }: UsageDetailProps) {
  const [refreshing, setRefreshing] = useState(false);
  const refreshStartRef = useRef<number>(0);
  // Open-count gating is centralized in config (bounded writes: stops at MAX)
  // so the intro hint fades out consistently across clients, not per-browser.
  const showIntroHints = useMemo(() => {
    const opens = (getCachedConfig()?.ui?.usage_intro_hint_opens as number | undefined) ?? 0;
    if (opens < INTRO_HINT_MAX_OPENS) void setUiSettings({ usage_intro_hint_opens: opens + 1 });
    return opens < INTRO_HINT_MAX_OPENS;
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
  const metrics = visibleUsageMetrics(snapshot);

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

      {isStale && <StaleBanner snapshot={snapshot} now={now} />}

      {metrics.map((metric) => (
        <UsageGauge
          key={metric.key}
          label={metric.label}
          variant={metric.key}
          pct={metric.pct}
          resetsAt={metric.resetsAt}
          state={metric.key === "session" ? sessionState : weeklyState}
          markers={metric.key === "session" ? sessionMarkers : weeklyMarkers}
          now={now}
          showIntroHint={showIntroHints}
        />
      ))}

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

// the cycle spend and reset instead of the gauge/projection machinery.
export function UsageSpendDetail({ snapshot, flipUp, onRefresh }: UsageDetailProps) {
  const [refreshing, setRefreshing] = useState(false);
  const refreshStartRef = useRef<number>(0);
  const now = Date.now();
  const isStale = now - snapshot.captured_at > STALE_MS;
  const remainingCents = snapshot.spend_remaining_cents;
  const limitCents = snapshot.spend_limit_cents;
  const hasQuota = remainingCents != null || limitCents != null;

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

      {isStale && <StaleBanner snapshot={snapshot} now={now} />}

      <div className="ct-usage-spend">
        <span className="amount">{formatSpend(snapshot.spend_cents ?? 0)}</span>
        <span className="caption">spent this cycle</span>
        {hasQuota && (
          <div className="quota">
            <span>{remainingCents != null ? formatSpend(remainingCents) : "—"} left</span>
            <span>{limitCents != null ? `${formatSpend(limitCents)} limit` : "limit —"}</span>
          </div>
        )}
        <span className="reset">
          {snapshot.spend_resets_at ? formatResetsAt(snapshot.spend_resets_at, now) : "—"}
        </span>
      </div>

      <div className="ct-usage-foot">
        <span className="live">
          <span className="pulse" />
          <span>last updated {formatRelative(now, snapshot.captured_at)}</span>
        </span>
      </div>
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
