import type { DeliveryState } from "./chatDeliveryModel";

export function ChatDeliveryState({ state, onRetry }: { state: DeliveryState; onRetry: () => void }) {
  if (state === "delivered") return null;
  return <div role="status" aria-live="polite" className="flex items-center justify-end gap-1.5 pt-1 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
    {state === "sending" && <><span aria-hidden="true" className="motion-safe:animate-pulse">· · ·</span><span className="sr-only">Sending</span></>}
    {state === "queued" && <span>Queued until the agent is ready</span>}
    {state === "failed" && <><span aria-label="Not delivered">!</span><button type="button" onClick={onRetry} className="rounded-full px-2 py-0.5 hover:bg-[var(--ct-hover)] focus-visible:outline focus-visible:outline-1">Retry</button></>}
  </div>;
}
