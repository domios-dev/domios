import {hasExpiringCredits} from "./creditExpiry";
import {uid} from "../../lib/uid";
import {invoke} from "../../lib/bridge";
import {DEV_BUILD} from "../../lib/devBuild";
import {resetCreditsForTab, type ResetResponse} from "./resetCreditsModel";
import {ResetCreditsSection} from "./ResetCreditsSection";
import { ownRuntimeUsage, latestUsageSnapshot, runtimeTokenLabel, runtimeTokenBreakdown, runtimeAccountCreditsLabel } from "./runtimeUsage";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useUsageSnapshot } from "../../hooks/useUsageSnapshot";
import { useProviderModelUsageState } from "../../hooks/useProviderModelUsage";
import { usageForModel } from "../../lib/modelGroups";
import type { ModelGroup } from "../../lib/types";
import { classify } from "../../types/usage";
import { formatSpend } from "./format";
import { groupSpendSnapshot, hasGroupSpend } from "./groupSpend";
import { UsageDetail, UsageSpendDetail } from "./UsageDetail";
import { UsageHistoryModal } from "./UsageHistoryModal";
import type {UsageSnapshot} from "../../types/usage";
import { unreportedBillingLabel, usageMetricsAriaLabel, visibleUsageMetrics, usagePillText, type VisibleUsageMetric } from "./metrics";
import { formatAge } from "../../lib/timeLabels";
import { AnchoredPopover } from "../shared/AnchoredPopover";

interface UsageBubbleProps {
  /** Provider id — required, no default. The component is hosted by a
      tab that already knows its provider via `usageProvider`. */
  tabId?: string;
  threadId?: string | null;
  provider: string;
  accountId?: string | null;
  /// Pass-through from a single top-level `useConfig()` so every tab's
  /// bubble doesn't re-fetch + re-listen on its own. Default `false`
  /// keeps callers that haven't been migrated rendering nothing.
  trackingEnabled?: boolean;
  /// The tab's selected model, used to pick its provider-declared group's
  /// spend/limit when the provider publishes per-group usage.
  modelId?: string | null;
  /// Provider-declared model groups, so a group-spend capsule matches by id
  /// without core knowing any vendor.
  modelGroups?: ModelGroup[];
}

const ONE_HOUR_MS = 60 * 60 * 1000;
const STALE_MS = 20 * 60 * 1000;

export function UsageBubble({
  provider,
  tabId,
  threadId,
  accountId = null,
  trackingEnabled = false,
  modelId = null,
  modelGroups,
}: UsageBubbleProps) {
  // The group's provider-declared spend meter takes precedence over the
  // provider-scoped polled snapshot: it is the tab's own upstream group.
  const { usage: modelUsage, refresh: refreshModelUsage } = useProviderModelUsageState(
    provider,
    trackingEnabled,
  );
  const groupUsage = modelId ? usageForModel(modelUsage, modelGroups, modelId) : null;
  const groupSnapshot = useMemo(
    () => (hasGroupSpend(groupUsage) ? groupSpendSnapshot(provider, groupUsage, Date.now()) : null),
    [provider, groupUsage],
  );
  const { snapshot, refresh, refetch } = useUsageSnapshot({
    provider,
    accountId,
    enabled: trackingEnabled,
  });
  const runtime = ownRuntimeUsage(snapshot,tabId,threadId);
  const active = runtime || snapshot?.runtime_usage
    ? latestUsageSnapshot(snapshot, tabId, threadId)
    : groupSnapshot ?? snapshot;
  const accountCreditsLabel = runtimeAccountCreditsLabel(runtime?.account_credits);
  const [, updateAge] = useState(0);
  const refreshAge = () => updateAge(value => value + 1);
  useEffect(() => {
    const refresh = () => updateAge(value => value + 1);
    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", refresh);
    return () => {
      window.removeEventListener("focus", refresh);
      document.removeEventListener("visibilitychange", refresh);
    };
  }, []);
  const tokenSession = runtime?.billing_mode === "api" ? runtime : null;
  const resetModel = tabId ? resetCreditsForTab(tabId, uid, async (key, creditId) => {
    const result = await invoke<ResetResponse>("agent_runtime_reset_credits", {tabId,request:{action:"redeem",confirmed:true,idempotencyKey:key,...(creditId ? {creditId} : {})}});
    try {
      await invoke("agent_runtime_reset_credits", {tabId,request:{action:"read"}});
      const updated = await invoke<UsageSnapshot|null>("usage_latest", {provider,accountId:accountId ?? undefined});
      await refetch();
      return {...result,remainingCount:ownRuntimeUsage(updated,tabId,threadId)?.resets?.availableCount ?? null};
    } catch {
      return result;
    }
  }) : null;
  const [open, setOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const closeDetail = useCallback(() => setOpen(false), []);

  if (!trackingEnabled || !active) return null;
  const now = Date.now();
  const ageMs = now - active.captured_at;
  if (!active.parsed_ok && ageMs > ONE_HOUR_MS) return null;

  const isStale = ageMs > STALE_MS;
  // to threshold against — render the cycle spend as a neutral pill.
  const isSpend = active.spend_cents != null && active.session_pct == null && active.weekly_pct == null;
  const spendLabel = active.spend_remaining_cents != null
    ? `${formatSpend(active.spend_remaining_cents)} left`
    : formatSpend(active.spend_cents ?? 0);
  const sessionState = classify(active.session_pct);
  const weeklyState = classify(active.weekly_pct);
  const metrics = visibleUsageMetrics(active);
  const billingLabel = unreportedBillingLabel(active);
  const worst: "neutral" | "warn" | "danger" = isSpend
    ? "neutral"
    : sessionState === "danger" || weeklyState === "danger"
      ? "danger"
      : sessionState === "warn" || weeklyState === "warn"
        ? "warn"
        : "neutral";

  const refreshUsage = async () => {
    if (groupSnapshot) {
      refreshModelUsage();
      return;
    }
    await refresh();
    await refetch();
  };

  const bubbleClass = [
    "ct-bar-pill ct-usage-bubble",
    worst === "warn" ? "is-warn" : "",
    worst === "danger" ? "is-danger" : "",
    isStale ? "is-stale" : "",
    open ? "is-open" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      onPointerEnter={refreshAge}
      onFocus={refreshAge}
      className="ct-usage"
      style={{ position: "relative", display: "inline-flex" }}
    >
      <button
        ref={btnRef}
        type="button"
        className={bubbleClass}
        aria-label={
          tokenSession ? `usage ${runtimeTokenLabel(tokenSession)}` : billingLabel ?? (isSpend
            ? `usage ${spendLabel}`
            : usageMetricsAriaLabel(metrics))
        }
        title={tokenSession ? runtimeTokenBreakdown(tokenSession) : undefined}
        onClick={(e) => {
          e.stopPropagation();
          if (!open && runtime && tabId) {
            void invoke("agent_runtime_reset_credits", {tabId,request:{action:"read"}}).then(() => refetch()).catch((error: unknown) => {
              if (DEV_BUILD) console.debug("Background reset credits refresh failed", error);
            });
          }
          setOpen((v) => !v);
        }}
      >
        {tokenSession ? <span>{runtimeTokenLabel(tokenSession)}</span> : isSpend ? (
          <span className="glyph neutral">
            <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
              <path d="M6 1.6v8.8M8 3.2H4.9a1.4 1.4 0 0 0 0 2.8h2.2a1.4 1.4 0 0 1 0 2.8H3.8" />
            </svg>
            <span className="pct">{spendLabel}</span>
          </span>
        ) : metrics.length ? (
          metrics.map((metric, index) => (
            <UsageMetricGlyph key={metric.key} metric={metric} state={metric.key === "session" ? sessionState : weeklyState} separated={index > 0} />
          ))
        ) : (
          <span>{usagePillText(active)}</span>
        )}
        {hasExpiringCredits(active.resets, active.usage_details, now) && <span className="ct-usage-credit-alert" role="img" aria-label="Unused credits expire within 3 days" />}
        {isStale && <span className="stale-age">·{formatAge(active.captured_at, now)}</span>}
      </button>

      <AnchoredPopover open={open} anchor={btnRef} placement="bottom-end" className="ct-usage" onClose={closeDetail}>
      {tokenSession && <div role="status" className="ct-usage-detail">{runtimeTokenBreakdown(tokenSession)}</div>}

      {!tokenSession && isSpend && (
        <UsageSpendDetail
          snapshot={active}
          accountCreditsLabel={accountCreditsLabel}
          resetSection={tabId && runtime && resetModel ? <ResetCreditsSection resets={active.resets} model={resetModel} /> : undefined}
          onRefresh={refreshUsage}
          onOpenHistory={() => {
            setHistoryOpen(true);
            setOpen(false);
          }}
        />
      )}

      {!tokenSession && !isSpend && (
        <UsageDetail
          snapshot={active}
          accountCreditsLabel={accountCreditsLabel}
          resetSection={tabId && runtime && resetModel ? <ResetCreditsSection resets={active.resets} model={resetModel} /> : undefined}
          onRefresh={refreshUsage}
          onOpenHistory={() => {
            setHistoryOpen(true);
            setOpen(false);
          }}
        />
      )}
      </AnchoredPopover>

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

function UsageMetricGlyph({metric, state, separated}: {metric: VisibleUsageMetric; state: "neutral" | "warn" | "danger"; separated: boolean}) {
  return <>
    {separated && <span className="sep" />}
    <span className={`glyph ${state}`}>
      <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
        {metric.key === "session" ? <><circle cx="6" cy="6" r="4.6" /><path d="M6 3.4V6l1.7 1.1" /></> : <path d="M1 7.2 3 5l2.2 2.2L7.4 4l3.4 3.4" />}
      </svg>
      <span className="pct">{Math.round(metric.pct)}%</span>
    </span>
  </>;
}
