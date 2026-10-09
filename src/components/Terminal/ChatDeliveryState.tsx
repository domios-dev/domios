import type { DeliveryState } from "./chatDeliveryModel";

const LABEL: Record<Exclude<DeliveryState, "delivered">, string> = {
  sending: "Sending",
  queued: "Queued until the agent is ready",
  failed: "Not delivered",
};

export function ChatDeliveryState({ state, onRetry }: { state: DeliveryState; onRetry: () => void }) {
  if (state === "delivered") return null;
  return <span role="status" aria-live="polite" className="ct-delivery" data-state={state} title={LABEL[state]}>
    <span className="ct-delivery__dot" aria-hidden="true" />
    <span className="sr-only">{LABEL[state]}</span>
    {state === "failed" && <button type="button" className="ct-delivery__retry" onClick={onRetry}>Retry</button>}
  </span>;
}
