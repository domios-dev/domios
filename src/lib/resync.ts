/**
 * Overflow full-resync signal (web). Events whose payloads are deltas or
 * filtered per-consumer can't be safely re-dispatched with a fabricated
 * payload — instead, hooks register their own initial loader here and re-run
 * it when the event stream reports loss (`overflow` frame in webEvents).
 */
import type { ResyncReason } from "./resyncModel";

const listeners = new Set<(reason: ResyncReason) => void>();

export function onResync(fn: (reason: ResyncReason) => void): () => void {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}

export function fireResync(reason: ResyncReason = "overflow"): void {
  for (const fn of [...listeners]) {
    try {
      fn(reason);
    } catch (e) {
      console.warn("[resync] listener threw:", e);
    }
  }
}
