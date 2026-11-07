import type { DeliveryState } from "./chatDeliveryModel";

const LABEL: Record<Exclude<DeliveryState, "delivered">, string> = {
  sending: "Sending",
  written: "Written to the agent — waiting for transcript confirmation; a busy agent will read after its current step",
  queued: "Queued for the agent",
  uncertain: "Submission unconfirmed",
  failed: "Not delivered",
};

export function ChatDeliveryState({ state, error, onRetry }: { state: DeliveryState; error?: string | null; onRetry?: () => void }) {
  if (state === "delivered") return null;
  const refusalMarker = "runtime_rejected_before_write:";
  const refusal = state === "failed" && error?.includes(refusalMarker)
    ? error.slice(error.indexOf(refusalMarker) + refusalMarker.length).trim() : null;
  const label = refusal || LABEL[state];
  return <span role="status" aria-live="polite" className="ct-delivery" data-state={state} title={label}>
    <span className="ct-delivery__dot" aria-hidden="true" />
    <span className={refusal ? undefined : "sr-only"}>{label}</span>
    {state === "failed" && onRetry && <button type="button" className="ct-delivery__retry" onClick={onRetry}>Retry</button>}
  </span>;
}
