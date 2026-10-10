import { useEffect, useSyncExternalStore } from "react";

/** Read-only view of host state a block renders from; `read` must return a stable value until the state changes. */
export interface LiveBindingSource<T> {
  subscribe(listener: () => void): () => void;
  read(key: string): T;
  /** Re-reads the real state without mutating it. */
  refresh(key: string): void;
}

/** Binds a block to live host state: re-reads on mount, on key change and whenever the host signals a change. */
export function useLiveBinding<T>(source: LiveBindingSource<T> | undefined, key: string, fallback: T): T {
  const value = useSyncExternalStore(
    source ? source.subscribe : noopSubscribe,
    () => (source ? source.read(key) : fallback),
    () => (source ? source.read(key) : fallback),
  );
  useEffect(() => {
    source?.refresh(key);
  }, [source, key]);
  return value;
}

function noopSubscribe() {
  return () => {};
}
