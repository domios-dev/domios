import { useEffect, useRef, useState } from "react";
import { useUsageSnapshot } from "../../hooks/useUsageSnapshot";
import { classify } from "../../types/usage";
import { formatSpend } from "./format";
import { UsageDetail, UsageSpendDetail } from "./UsageDetail";
import { UsageHistoryModal } from "./UsageHistoryModal";
import { usageMetricsAriaLabel, visibleUsageMetrics, type VisibleUsageMetric } from "./metrics";

interface UsageBubbleProps {
  /** Provider id — required, no default. The component is hosted by a
      tab that already knows its provider via `usageProvider`. */
  provider: string;
  accountId?: string | null;
  /// Pass-through from a single top-level `useConfig()` so every tab's
  /// bubble doesn't re-fetch + re-listen on its own. Default `false`
  /// keeps callers that haven't been migrated rendering nothing.
  trackingEnabled?: boolean;
}

const ONE_HOUR_MS = 60 * 60 * 1000;
const STALE_MS = 20 * 60 * 1000;

export function UsageBubble({ provider, accountId = null, trackingEnabled = false }: UsageBubbleProps) {
  // nor register a usage-snapshot-updated listener (was a per-tab IPC fan-out).
  const { snapshot, refresh, refetch } = useUsageSnapshot({ provider, accountId, enabled: trackingEnabled });
  const [open, setOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [flipUp, setFlipUp] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const btnRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDocClick = (e: MouseEvent) => {
      if (!wrapRef.current) return;
      if (!wrapRef.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("mousedown", onDocClick);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDocClick);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  useEffect(() => {
    if (!open || !btnRef.current) return;
    const btnRect = btnRef.current.getBoundingClientRect();
    const tab = btnRef.current.closest("[data-tab-id]") as HTMLElement | null;
    const tabRect = tab?.getBoundingClientRect();
    const room = tabRect ? tabRect.bottom - btnRect.bottom : window.innerHeight - btnRect.bottom;
    setFlipUp(room < 360);
  }, [open]);

  if (!trackingEnabled) return null;
  if (!snapshot) return null;
  const ageMs = Date.now() - snapshot.captured_at;
  if (!snapshot.parsed_ok && ageMs > ONE_HOUR_MS) return null;

  const isStale = ageMs > STALE_MS;
  // to threshold against — render the cycle spend as a neutral pill.
  const isSpend = snapshot.spend_cents != null && snapshot.session_pct == null && snapshot.weekly_pct == null;
  const spendLabel = snapshot.spend_remaining_cents != null
    ? `${formatSpend(snapshot.spend_remaining_cents)} left`
    : formatSpend(snapshot.spend_cents ?? 0);
  const sessionState = classify(snapshot.session_pct);
  const weeklyState = classify(snapshot.weekly_pct);
  const metrics = visibleUsageMetrics(snapshot);
  const worst: "neutral" | "warn" | "danger" = isSpend
    ? "neutral"
    : sessionState === "danger" || weeklyState === "danger"
      ? "danger"
      : sessionState === "warn" || weeklyState === "warn"
        ? "warn"
        : "neutral";

  const bubbleClass = [
    "ct-usage-bubble",
    worst === "warn" ? "is-warn" : "",
    worst === "danger" ? "is-danger" : "",
    isStale ? "is-stale" : "",
    open ? "is-open" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      ref={wrapRef}
      className="ct-usage"
      style={{ position: "relative", display: "inline-flex" }}
    >
      <button
        ref={btnRef}
        type="button"
        className={bubbleClass}
        aria-label={
          isSpend
            ? `usage ${spendLabel}`
            : usageMetricsAriaLabel(metrics)
        }
        onClick={(e) => {
          e.stopPropagation();
          setOpen((v) => !v);
        }}
      >
        {isSpend ? (
          <span className="glyph neutral">
            <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
              <path d="M6 1.6v8.8M8 3.2H4.9a1.4 1.4 0 0 0 0 2.8h2.2a1.4 1.4 0 0 1 0 2.8H3.8" />
            </svg>
            <span className="pct">{spendLabel}</span>
          </span>
        ) : (
          metrics.map((metric, index) => (
            <UsageMetricGlyph
              key={metric.key}
              metric={metric}
              state={metric.key === "session" ? sessionState : weeklyState}
              separated={index > 0}
            />
          ))
        )}
        {isStale && <span className="stale-age">·{formatAgeShort(ageMs)}</span>}
      </button>

      {open && isSpend && (
        <UsageSpendDetail
          snapshot={snapshot}
          flipUp={flipUp}
          onRefresh={async () => {
            await refresh();
            await refetch();
          }}
          onOpenHistory={() => {
            setHistoryOpen(true);
            setOpen(false);
          }}
        />
      )}

      {open && !isSpend && (
        <UsageDetail
          snapshot={snapshot}
          flipUp={flipUp}
          onRefresh={async () => {
            await refresh();
            await refetch();
          }}
          onOpenHistory={() => {
            setHistoryOpen(true);
            setOpen(false);
          }}
        />
      )}

      {historyOpen && (
        <UsageHistoryModal
          provider={provider}
          accountId={accountId}
          onClose={() => setHistoryOpen(false)}
        />
      )}
    </div>
  );
}

function UsageMetricGlyph({
  metric,
  state,
  separated,
}: {
  metric: VisibleUsageMetric;
  state: "neutral" | "warn" | "danger";
  separated: boolean;
}) {
  return (
    <>
      {separated && <span className="sep" />}
      <span className={`glyph ${state}`}>
        <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
          {metric.key === "session" ? (
            <>
              <circle cx="6" cy="6" r="4.6" />
              <path d="M6 3.4V6l1.7 1.1" />
            </>
          ) : (
            <path d="M1 7.2 3 5l2.2 2.2L7.4 4l3.4 3.4" />
          )}
        </svg>
        <span className="pct">{formatPct(metric.pct)}</span>
      </span>
    </>
  );
}

function formatPct(pct: number | null | undefined): string {
  if (pct == null) return "—";
  return `${Math.round(pct)}%`;
}

function formatAgeShort(ms: number): string {
  const m = Math.floor(ms / 60_000);
  if (m < 60) return `${m}m`;
  return `${Math.floor(m / 60)}h`;
}
