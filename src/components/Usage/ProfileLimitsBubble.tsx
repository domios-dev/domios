import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { useUsageSnapshot } from "../../hooks/useUsageSnapshot";
import { formatAge } from "../../lib/timeLabels";
import { classify } from "../../types/usage";
import { contextMeter } from "../Terminal/composerModels";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { balanceFailure, balanceLabel, balanceNeedsRead, balanceReading } from "./balanceModel";
import { ContextUsageBubble } from "./ContextUsageBubble";
import { contextUsage, useClaimContext } from "./contextUsage";
import { quotaPill, quotaReadDue, quotaReading, unavailablePill } from "./quotaModel";
import { ownRuntimeUsage } from "./runtimeUsage";
import { UsageGauge } from "./UsageGauge";

function LimitsPill({ label, text, tone, children }: { label: string; text: string; tone?: "quiet" | "warn" | "danger"; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  return (
    <div className="ct-usage" style={{ position: "relative", display: "inline-flex" }}>
      <button ref={anchor} type="button" aria-label={label} aria-expanded={open}
        className={["ct-bar-pill ct-usage-bubble", tone ? `is-${tone}` : "", open ? "is-open" : ""].filter(Boolean).join(" ")}
        onClick={(event) => { event.stopPropagation(); setOpen((value) => !value); }}>
        <span className="pct">{text}</span>
      </button>
      <AnchoredPopover open={open} anchor={anchor} placement="bottom-end" className="ct-usage" onClose={close}>
        <div className="ct-usage-popover" role="dialog" aria-label="usage details">{children}</div>
      </AnchoredPopover>
    </div>
  );
}

/** The tab's own session, whose reported context stands in when the account's limits cannot be read. */
export interface TabUsageSession { provider: string; tabId: string; threadId?: string | null; context: unknown; label?: string }

/** Nothing readable for the account: the session's reported context when it has one, otherwise a dash that says why. */
export function UnavailableLimitsBubble({ reason, session }: { reason: string; session?: TabUsageSession }) {
  const { snapshot } = useUsageSnapshot({ provider: session?.provider ?? "", enabled: session !== undefined });
  const runtime = ownRuntimeUsage(snapshot, session?.tabId, session?.threadId);
  const usage = session ? contextUsage(session.context, runtime) : null;
  useClaimContext(session?.tabId, usage !== null);
  if (session && usage) {
    return <ContextUsageBubble title={`${session.label ?? session.provider} usage`} usage={usage} reason={reason}
      capturedAt={runtime?.token_usage?.captured_at ?? runtime?.captured_at ?? null} />;
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
  if (!trackingEnabled) return null;
  const reading = quotaReading(snapshot);
  const failure = balanceFailure(snapshot, error, "Usage limits are unavailable");
  if (!reading) return <UnavailableLimitsBubble reason={failure ?? "Usage limits have not been read yet"} session={session} />;
  const pill = quotaPill(reading, failure);
  const state = classify(pill.percent);
  const now = Date.now();
  return (
    <LimitsPill label={pill.label} text={pill.text} tone={state === "neutral" ? (failure ? "warn" : undefined) : state}>
      <div className="head"><span className="title">{reading.title}</span></div>
      {reading.limits.map((limit) => (
        <UsageGauge key={limit.label} label={limit.label} variant="session" pct={limit.usedPercent} resetsAt={limit.resetsAt} state={classify(limit.usedPercent)} now={now} />
      ))}
      {failure ? <p className="ct-usage-detail">Last read failed: {failure}</p> : null}
      <div className="ct-usage-foot">
        <span>{snapshot?.captured_at !== undefined ? `last updated ${formatAge(snapshot.captured_at, now)}` : "not read yet"}</span>
        <button type="button" className="link" onClick={() => void refresh()}>Refresh</button>
      </div>
    </LimitsPill>
  );
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
