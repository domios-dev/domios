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
  return <span role="status" aria-live="polite" aria-label={label}
    className={inline ? "ct-delivery ct-delivery--inline" : "ct-delivery"} data-state={state} title={label}>
    <span className="ct-delivery__dot" aria-hidden="true" />
    {(inline || state === "failed" || state === "uncertain") && <span className="ct-delivery__label">{label}</span>}
    {!inline && state === "queued" && queuedBehindTurn && <span className="ct-delivery__label">Queued</span>}
    {state === "failed" && onRetry && <button type="button" className="ct-delivery__retry" onClick={onRetry}>Retry</button>}
    {state === "uncertain" && onEdit && <button type="button" className="ct-delivery__retry" onClick={onEdit}>Edit and resend</button>}
    {(state === "failed" || state === "uncertain") && onRemove && <button type="button" className="ct-delivery__retry" onClick={onRemove}>Remove</button>}
  </span>;
}
