import type { DeliveryState } from "./chatDeliveryModel";

const LABEL: Record<Exclude<DeliveryState, "delivered">, string> = {
  sending: "Sending",
  written: "Awaiting confirmation",
  queued: "Queued for the agent",
  uncertain: "Delivery unconfirmed; may have reached the agent",
  failed: "Not delivered",
};
const QUEUED_BEHIND_TURN = "Queued until the current turn ends";
const RESEND_WITH_CLI = "Not delivered. Resend with domios send";

export function ChatDeliveryState({ state, error, onRetry, onRemove, onEdit, resendWithCli = false, queuedBehindTurn = false, inline = false }: {
  state: DeliveryState; error?: string | null; onRetry?: () => void; onRemove?: () => void; onEdit?: () => void; resendWithCli?: boolean; queuedBehindTurn?: boolean; inline?: boolean;
}) {
  if (state === "delivered") return null;
  const refusalMarker = state === "failed" ? "runtime_rejected_before_write:"
    : state === "uncertain" ? "agent_refused_while_busy:" : null;
  const refusal = refusalMarker && error?.includes(refusalMarker)
    ? error.slice(error.indexOf(refusalMarker) + refusalMarker.length).trim() : null;
  const resendHint = state === "failed" && resendWithCli;
  const label = refusal || (resendHint ? RESEND_WITH_CLI : state === "queued" && queuedBehindTurn ? QUEUED_BEHIND_TURN : LABEL[state]);
  return <span role="status" aria-live="polite" aria-label={label}
    className={inline ? "ct-delivery ct-delivery--inline" : "ct-delivery"} data-state={state} title={label}>
    <span className="ct-delivery__dot" aria-hidden="true" />
    <span className="ct-delivery__label">{label}</span>
    {state === "failed" && onRetry && <button type="button" className="ct-delivery__retry" onClick={onRetry}>Retry</button>}
    {state === "uncertain" && onEdit && <button type="button" className="ct-delivery__retry" onClick={onEdit}>Edit and resend</button>}
    {onRemove && <button type="button" className="ct-delivery__retry" onClick={onRemove}>Remove</button>}
  </span>;
}
