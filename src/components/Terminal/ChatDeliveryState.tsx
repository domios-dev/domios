import { useRef, useState } from "react";
import { AnchoredPopover } from "../shared/AnchoredPopover";
import type { DeliveryState } from "./chatDeliveryModel";

const LABEL: Record<Exclude<DeliveryState, "delivered">, string> = {
  sending: "Sending",
  written: "Awaiting confirmation",
  queued: "Queued for the agent",
  uncertain: "Delivery unconfirmed; may have reached the agent",
  failed: "Not delivered",
};
const QUEUED_BEHIND_TURN = "Queued until the current turn ends";
const RESEND_WITH_CLI = "Resend with domios send.";

export function ChatDeliveryState({ state, error, onRetry, onRemove, onEdit, resendWithCli = false, queuedBehindTurn = false, inline = false }: {
  state: DeliveryState; error?: string | null; onRetry?: () => void; onRemove?: () => void; onEdit?: () => void; resendWithCli?: boolean; queuedBehindTurn?: boolean; inline?: boolean;
}) {
  if (state === "delivered") return null;
  const refusalMarker = state === "failed" ? ["runtime_rejected_before_write:", "runtime_preflight_before_write:"].find(marker => error?.includes(marker))
    : state === "uncertain" ? "agent_refused_while_busy:" : null;
  const refusal = refusalMarker && error?.includes(refusalMarker)
    ? error.slice(error.indexOf(refusalMarker) + refusalMarker.length).trim() : null;
  const resendHint = state === "failed" && resendWithCli;
  const failure = state === "failed" ? error?.trim() || "Message was not sent; the failure reason is unavailable." : null;
  const reason = refusal || failure || (state === "queued" && queuedBehindTurn ? QUEUED_BEHIND_TURN : LABEL[state]);
  const label = resendHint ? `${reason} ${RESEND_WITH_CLI}` : reason;
  const actions = state === "failed" || state === "uncertain"
    ? [{ label: "Retry", run: state === "failed" ? onRetry : undefined },
      { label: "Edit and resend", run: state === "uncertain" ? onEdit : undefined },
      { label: "Remove", run: onRemove }].filter(action => action.run)
    : [];
  return <span role="status" aria-live="polite" aria-label={label}
    className={inline ? "ct-delivery ct-delivery--inline" : "ct-delivery"} data-state={state} title={label}>
    {!inline && actions.length > 0 ? <DeliveryActions key={state} label={label} actions={actions} />
      : <span className="ct-delivery__dot" aria-hidden="true" />}
    {inline && <>
      <span className="ct-delivery__label">{label}</span>
      {actions.map(action => <button key={action.label} type="button" className="ct-delivery__retry" onClick={action.run}>{action.label}</button>)}
    </>}
  </span>;
}

function DeliveryActions({ label, actions }: { label: string; actions: { label: string; run?: () => void }[] }) {
  const anchor = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  return <>
    <button ref={anchor} type="button" className="ct-delivery__trigger" aria-label={label} title={label}
      aria-haspopup="dialog" aria-expanded={open} onClick={() => setOpen(value => !value)}>
      <span className="ct-delivery__dot" aria-hidden="true" />
    </button>
    <AnchoredPopover open={open} anchor={anchor} onClose={() => setOpen(false)}>
      <div role="dialog" aria-label="Delivery actions" className="ct-menu" onBlur={event => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null) && event.relatedTarget !== anchor.current) setOpen(false);
      }}>
        {actions.map((action, index) => <button key={action.label} type="button" className="ct-menu-row"
          ref={index === 0 ? button => { button?.focus({ preventScroll: true }); } : undefined}
          onClick={() => { setOpen(false); anchor.current?.focus({ preventScroll: true }); action.run?.(); }}>{action.label}</button>)}
      </div>
    </AnchoredPopover>
  </>;
}
