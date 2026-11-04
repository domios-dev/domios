import type { DeliveryState } from "./chatDeliveryModel";

const LABEL: Record<Exclude<DeliveryState, "delivered">, string> = {
  sending: "Sending",
  written: "Written to the agent — waiting for transcript confirmation; a busy agent will read after its current step",
  queued: "Queued for the agent",
  uncertain: "Submission unconfirmed",
  failed: "Not delivered",
};

export function ChatDeliveryState({ state, busyRefusal, onRetry }: { state: DeliveryState; busyRefusal?: boolean; onRetry?: () => void }) {
  if (state === "delivered") return null;
  const label = state === "failed" && busyRefusal ? "Not delivered: the agent was busy" : LABEL[state];
  return <span role="status" aria-live="polite" className="ct-delivery" data-state={state} title={label}>
    <span className="ct-delivery__dot" aria-hidden="true" />
    <span className={state === "failed" && busyRefusal ? undefined : "sr-only"}>{label}</span>
    {state === "failed" && onRetry && <button type="button" className="ct-delivery__retry" onClick={onRetry}>Retry</button>}
  </span>;
}
