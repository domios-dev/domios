import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { useUsageSnapshot } from "../../hooks/useUsageSnapshot";
import { formatAge } from "../../lib/timeLabels";
import { classify } from "../../types/usage";
import type { ModelGroup } from "../../lib/types";
import { contextMeter } from "../Terminal/composerModels";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { balanceFailure, balanceLabel, balanceNeedsRead, balanceReading } from "./balanceModel";
import { ContextUsageBubble } from "./ContextUsageBubble";
import { contextUsage, useClaimContext } from "./contextUsage";
import {
  activeQuotaReset,
  activeQuotaGroup,
  formatResetCountdown,
  nextQuotaResetTickMs,
  quotaPill,
  quotaGroups,
  quotaReadDue,
  quotaReading,
  quotaResetPill,
  unavailablePill,
  type QuotaReading,
  type QuotaGroup,
  type QuotaResetReading,
} from "./quotaModel";
import { ownRuntimeUsage, runtimeTokenBreakdown, runtimeTokenLabel } from "./runtimeUsage";
import { UsageGauge } from "./UsageGauge";
import { UsageMetricGlyph } from "./UsageMetricGlyph";

function LimitsPill({ label, text, tone, children }: { label: string; text: ReactNode; tone?: "quiet" | "warn" | "danger"; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  return (
    <div className="ct-usage" style={{ position: "relative", display: "inline-flex" }}>
      <button ref={anchor} type="button" aria-label={label} aria-expanded={open}
        className={["ct-bar-pill ct-usage-bubble", tone ? `is-${tone}` : "", open ? "is-open" : ""].filter(Boolean).join(" ")}
        onClick={(event) => { event.stopPropagation(); setOpen((value) => !value); }}>
        {typeof text === "string" ? <span className="pct">{text}</span> : text}
      </button>
      <AnchoredPopover open={open} anchor={anchor} placement="bottom-end" className="ct-usage" onClose={close}>
        <div className="ct-usage-popover" role="dialog" aria-label="usage details">{children}</div>
      </AnchoredPopover>
    </div>
  );
}

export function useQuotaResetTick(resetsAt: number | null, now: number): void {
  const [, setTick] = useState(0);
  useEffect(() => {
    if (resetsAt == null) return;
    const delay = nextQuotaResetTickMs(resetsAt, Date.now());
    if (delay == null) return;
    const timer = setTimeout(() => setTick((value) => value + 1), delay);
    return () => clearTimeout(timer);
  }, [resetsAt, now]);
}

export function QuotaReadingPill({ reading, failure, capturedAt, onRefresh, modelId, modelGroups, reset }: {
  reading: QuotaReading; failure: string | null; capturedAt?: number; onRefresh?: () => void;
  modelId?: string | null; modelGroups?: ModelGroup[]; reset?: QuotaResetReading | null;
}) {
  const groups = quotaGroups(reading);
  const active = activeQuotaGroup(groups, modelId, modelGroups);
  const pill = quotaPill(active ? { ...reading, limits: active.limits } : reading, failure);
  const state = groups.length && !active ? "neutral" : classify(pill.percent);
  const failureNote = failure ? `, last read failed: ${failure}` : "";
  const quotaLabel = active
    ? `${active.label} usage limits, ${active.metrics.map(metric => `${metric.key === "session" ? "5h" : "weekly"} ${Math.round(metric.pct)}% used`).join(", ")}${failureNote}`
    : groups.length ? `Active model quota unavailable${failureNote}` : pill.label;
  const now = Date.now();
  const currentReset = reset && reset.resetsAt > now ? reset : null;
  useQuotaResetTick(currentReset?.resetsAt ?? null, now);
  const resetNote = currentReset ? `Profile quota resets in ${formatResetCountdown(currentReset.resetsAt, now)}.` : null;
  return (
    <LimitsPill label={[quotaLabel, resetNote].filter(Boolean).join(". ")} text={active ? active.metrics.map((metric, index) => (
      <UsageMetricGlyph key={metric.key} metric={metric} state={classify(metric.pct)} separated={index > 0} />
    )) : groups.length ? "—" : pill.text} tone={state === "neutral" ? (failure || currentReset ? "warn" : undefined) : state}>
      <div className="head"><span className="title">{reading.title}</span></div>
      <QuotaReadingDetails key={active?.label} reading={reading} groups={groups} active={active} now={now} />
      {resetNote && <p className="ct-usage-detail">{resetNote} {currentReset?.reason}</p>}
      {failure ? <p className="ct-usage-detail">Last read failed: {failure}</p> : null}
      <div className="ct-usage-foot">
        <span>{capturedAt !== undefined ? `last updated ${formatAge(capturedAt, now)}` : "not read yet"}</span>
        {onRefresh ? <button type="button" className="link" onClick={onRefresh}>Refresh</button> : null}
      </div>
    </LimitsPill>
  );
}

function QuotaReadingDetails({ reading, groups, active, now }: {
  reading: QuotaReading; groups: QuotaGroup[]; active: QuotaGroup | null; now: number;
}) {
  const [selection, setSelection] = useState(active?.label ?? "");
  const selected = groups.find(group => group.label === selection);
  const limits = selected?.limits ?? reading.limits;
  const hasUngrouped = groups.reduce((count, group) => count + group.limits.length, 0) !== reading.limits.length;
  return <>
    {(groups.length > 1 || (groups.length > 0 && hasUngrouped)) && (
      <label className="ct-usage-detail flex items-center justify-between gap-2">Quota group
        <select value={selected?.label ?? ""} onChange={event => setSelection(event.target.value)}>
          <option value="">All limits</option>
          {groups.map(group => <option key={group.label}>{group.label}</option>)}
        </select>
      </label>
    )}
    {limits.map((limit, index) => (
      <UsageGauge key={`${limit.label}:${index}`} label={limit.label} variant={/ weekly$/i.test(limit.label) ? "weekly" : "session"}
        pct={limit.usedPercent} resetsAt={limit.resetsAt} state={classify(limit.usedPercent)} now={now} />
    ))}
  </>;
}

export function QuotaResetPill({ title, reset, note, now }: {
  title: string; reset: QuotaResetReading; note?: string; now: number;
}) {
  const pill = quotaResetPill(reset, now);
  return (
    <LimitsPill label={pill.label} text={pill.text} tone="warn">
      <div className="head"><span className="title">{title}</span></div>
      <UsageGauge label="Quota limit" variant="session" pct={100} resetsAt={reset.resetsAt} state="danger" now={now} />
      {reset.reason ? <p className="ct-usage-detail">{reset.reason}</p> : null}
      {note ? <p className="ct-usage-detail ct-usage-detail--note">{note}</p> : null}
      {reset.observedAt !== null ? (
        <div className="ct-usage-foot">
          <span>observed {formatAge(reset.observedAt, now)}</span>
        </div>
      ) : null}
    </LimitsPill>
  );
}

/** The tab's own session, whose reported context stands in when the account's limits cannot be read. */
export interface TabUsageSession { provider: string; accountId?: string | null; profileAccountId?: string | null; tabId: string; threadId?: string | null; context: unknown; label?: string; modelId?: string | null; modelGroups?: ModelGroup[] }

/** Nothing readable for the account: pushed quota or observed reset when present, else the session's reported context or a dash that says why. */
export function UnavailableLimitsBubble({ reason, session }: { reason: string; session?: TabUsageSession }) {
  const { snapshot } = useUsageSnapshot({ provider: session?.provider ?? "", accountId: session?.accountId, enabled: session !== undefined });
  const { snapshot: profileSnapshot } = useUsageSnapshot({
    provider: session?.provider ?? "",
    accountId: session?.profileAccountId,
    enabled: Boolean(session?.provider && session?.profileAccountId),
  });
  const now = Date.now();
  const pushedQuota = quotaReading(profileSnapshot);
  const quotaReset = activeQuotaReset(profileSnapshot, now);
  useQuotaResetTick(pushedQuota ? null : quotaReset?.resetsAt ?? null, now);
  const runtime = ownRuntimeUsage(snapshot, session?.tabId, session?.threadId);
  const usage = session && !pushedQuota && !quotaReset ? contextUsage(session.context, runtime) : null;
  useClaimContext(session?.tabId, usage !== null);
  if (pushedQuota) {
    return <QuotaReadingPill reading={pushedQuota} failure={null} capturedAt={profileSnapshot?.captured_at} modelId={session?.modelId}
      modelGroups={session?.modelGroups} reset={quotaReset} />;
  }
  if (quotaReset) {
    return <QuotaResetPill title={`${session?.label ?? session?.provider ?? "Account"} quota`} reset={quotaReset} note={reason} now={now} />;
  }
  if (session && usage) {
    return <ContextUsageBubble title={`${session.label ?? session.provider} usage`} usage={usage} reason={reason}
      capturedAt={runtime?.token_usage?.captured_at ?? runtime?.captured_at ?? null} />;
  }
  if (session && runtime?.token_usage) {
    const label = runtimeTokenLabel(runtime);
    return (
      <LimitsPill label={`usage ${label}`} text={label}>
        <div className="head"><span className="title">{session.label ?? session.provider} usage</span></div>
        <div role="status" className="ct-usage-detail">{runtimeTokenBreakdown(runtime)}</div>
        <p className="ct-usage-detail">{reason}</p>
      </LimitsPill>
    );
  }
  const pill = unavailablePill(reason);
  return (
    <LimitsPill label={pill.label} text={pill.text} tone="quiet">
      <div className="head"><span className="title">Usage limits</span></div>
      <p className="ct-usage-detail">{reason}</p>
    </LimitsPill>
  );
}

/** Account quota the provider plugin reads for this profile; reads follow the mount and the session's activity, never a timer. */
export function QuotaBubble({ provider, accountId, trackingEnabled, session }: { provider: string; accountId: string; trackingEnabled: boolean; session?: TabUsageSession }) {
  const { snapshot, error, refresh } = useUsageSnapshot({ provider, accountId, enabled: trackingEnabled });
  const lastRequested = useRef<number | null>(null);
  const reportedContext = contextMeter(session?.context)?.used ?? null;
  useEffect(() => {
    if (!trackingEnabled || !quotaReadDue(lastRequested.current, Date.now())) return;
    lastRequested.current = Date.now();
    void refresh();
  }, [trackingEnabled, reportedContext, refresh]);
  const now = Date.now();
  const reading = trackingEnabled ? quotaReading(snapshot) : null;
  const quotaReset = trackingEnabled ? activeQuotaReset(snapshot, now) : null;
  useQuotaResetTick(reading ? null : quotaReset?.resetsAt ?? null, now);
  if (!trackingEnabled) return null;
  const failure = balanceFailure(snapshot, error, "Usage limits are unavailable");
  if (reading) {
    return <QuotaReadingPill reading={reading} failure={failure} capturedAt={snapshot?.captured_at}
      modelId={session?.modelId} modelGroups={session?.modelGroups} reset={quotaReset} onRefresh={() => void refresh()} />;
  }
  if (quotaReset) {
    return <QuotaResetPill title={`${session?.label ?? provider} quota`} reset={quotaReset} note={failure ?? undefined} now={now} />;
  }
  return <UnavailableLimitsBubble reason={failure ?? "Usage limits have not been read yet"} session={session} />;
}

/** A balance the provider plugin reads for this profile; reads are requested, never polled. */
export function BalanceBubble({ provider, accountId, trackingEnabled }: { provider: string; accountId: string; trackingEnabled: boolean }) {
  const { snapshot, error, refresh } = useUsageSnapshot({ provider, accountId, enabled: trackingEnabled });
  const requested = useRef(false);
  const needsRead = balanceNeedsRead(snapshot, Date.now());
  useEffect(() => {
    if (!trackingEnabled || requested.current || !needsRead) return;
    requested.current = true;
    void refresh();
  }, [trackingEnabled, needsRead, refresh]);
  if (!trackingEnabled) return null;
  const reading = balanceReading(snapshot);
  const failure = balanceFailure(snapshot, error);
  const text = reading ? balanceLabel(reading) : failure ? "balance ?" : "balance …";
  const staleCause = reading && failure ? `, last read failed: ${failure}` : "";
  const label = reading ? `balance ${text}${reading.available ? "" : ", insufficient"}${staleCause}` : failure ? `balance unavailable: ${failure}` : "balance not read yet";
  return (
    <LimitsPill label={label} text={text} tone={failure || reading?.available === false ? "warn" : undefined}>
      <div className="head"><span className="title">Account balance</span></div>
      <p className="ct-usage-detail">{reading ? `${text}${reading.available ? "" : " — insufficient for new requests"}` : failure ?? "Reading the balance…"}</p>
      {reading && failure ? <p className="ct-usage-detail">Last read failed: {failure}</p> : null}
      <div className="ct-usage-foot">
        <span>{snapshot?.captured_at !== undefined ? `last updated ${formatAge(snapshot.captured_at, Date.now())}` : "not read yet"}</span>
        <button type="button" className="link" onClick={() => void refresh()}>Refresh</button>
      </div>
    </LimitsPill>
  );
}
