import type { DeliveryState } from "./chatDeliveryModel";

const LABEL: Record<Exclude<DeliveryState, "delivered">, string> = {
  sending: "Sending",
  written: "Written to the agent — waiting for transcript confirmation; a busy agent will read after its current step",
  queued: "Queued for the agent",
  uncertain: "Submission unconfirmed",
  failed: "Not delivered",
};
const QUEUED_BEHIND_TURN = "Queued until the current turn ends";

/** `inline` sits inside a card header and shows its label; beside a bubble it stays one quiet dot. */
export function ChatDeliveryState({ state, error, onRetry, queuedBehindTurn = false, inline = false }: {
  state: DeliveryState; error?: string | null; onRetry?: () => void; queuedBehindTurn?: boolean; inline?: boolean;
}) {
  if (state === "delivered") return null;
  const refusalMarker = state === "failed" ? "runtime_rejected_before_write:"
    : state === "uncertain" ? "agent_refused_while_busy:" : null;
  const refusal = refusalMarker && error?.includes(refusalMarker)
    ? error.slice(error.indexOf(refusalMarker) + refusalMarker.length).trim() : null;
  const label = refusal || (state === "queued" && queuedBehindTurn ? QUEUED_BEHIND_TURN : LABEL[state]);
  return <span role="status" aria-live="polite" className={inline ? "ct-delivery ct-delivery--inline" : "ct-delivery"} data-state={state} title={label}>
    <span className="ct-delivery__dot" aria-hidden="true" />
    <span className={refusal || inline ? "ct-delivery__label" : "sr-only"}>{label}</span>
    {state === "failed" && onRetry && <button type="button" className="ct-delivery__retry" onClick={onRetry}>Retry</button>}
  </span>;
}
