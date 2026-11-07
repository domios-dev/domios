import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { useUsageSnapshot } from "../../hooks/useUsageSnapshot";
import { formatAge } from "../../lib/timeLabels";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import { balanceLabel, balanceNeedsRead, balanceReading } from "./balanceModel";

function LimitsPill({ label, text, warn, children }: { label: string; text: string; warn?: boolean; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  return (
    <div className="ct-usage" style={{ position: "relative", display: "inline-flex" }}>
      <button ref={anchor} type="button" aria-label={label} aria-expanded={open}
        className={["ct-bar-pill ct-usage-bubble", warn ? "is-warn" : "", open ? "is-open" : ""].filter(Boolean).join(" ")}
        onClick={(event) => { event.stopPropagation(); setOpen((value) => !value); }}>
        <span className="pct">{text}</span>
      </button>
      <AnchoredPopover open={open} anchor={anchor} placement="bottom-end" className="ct-usage" onClose={close}>
        <div className="ct-usage-popover" role="dialog" aria-label="usage details">{children}</div>
      </AnchoredPopover>
    </div>
  );
}

/** The profile declares that its limits cannot be read; the bubble says why instead of showing nothing. */
export function UnavailableLimitsBubble({ reason }: { reason: string }) {
  return (
    <LimitsPill label={`usage limits unavailable: ${reason}`} text="limits n/a">
      <div className="head"><span className="title">Usage limits</span></div>
      <p className="ct-usage-detail">{reason}</p>
    </LimitsPill>
  );
}

/** A balance the provider plugin reads for this profile; reads are requested, never polled. */
export function BalanceBubble({ provider, accountId, trackingEnabled }: { provider: string; accountId: string; trackingEnabled: boolean }) {
  const { snapshot, refresh } = useUsageSnapshot({ provider, accountId, enabled: trackingEnabled });
  const requested = useRef(false);
  useEffect(() => {
    if (!trackingEnabled || requested.current || !balanceNeedsRead(snapshot, Date.now())) return;
    requested.current = true;
    void refresh();
  }, [trackingEnabled, snapshot, refresh]);
  if (!trackingEnabled) return null;
  const reading = balanceReading(snapshot);
  const failure = snapshot && !snapshot.parsed_ok ? snapshot.failure_reason ?? "Balance unavailable" : null;
  const text = reading ? balanceLabel(reading) : failure ? "balance ?" : "balance …";
  const label = reading ? `balance ${text}${reading.available ? "" : ", insufficient"}` : failure ? `balance unavailable: ${failure}` : "balance not read yet";
  return (
    <LimitsPill label={label} text={text} warn={Boolean(failure) || reading?.available === false}>
      <div className="head"><span className="title">Account balance</span></div>
      <p className="ct-usage-detail">{reading ? `${text}${reading.available ? "" : " — insufficient for new requests"}` : failure ?? "Reading the balance…"}</p>
      <div className="ct-usage-foot">
        <span>{snapshot ? `last updated ${formatAge(snapshot.captured_at, Date.now())}` : "not read yet"}</span>
        <button type="button" className="link" onClick={() => void refresh()}>Refresh</button>
      </div>
    </LimitsPill>
  );
}
