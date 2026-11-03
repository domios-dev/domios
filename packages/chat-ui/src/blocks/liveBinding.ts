import { createContext, createElement, useContext, useEffect, useRef, useSyncExternalStore, type ReactNode } from "react";

const LiveBindingActivity = createContext(true);

export function ChatBlockActivityProvider({ active, children }: { active: boolean; children: ReactNode }) {
  return createElement(LiveBindingActivity.Provider, { value: active }, children);
}

/** Read-only view of host state a block renders from; `read` must return a stable value until the state changes. */
export interface LiveBindingSource<T> {
  subscribe(listener: () => void): () => void;
  read(key: string): T;
  /** Re-reads the real state without mutating it. */
  refresh(key: string): void;
  retain?(key: string): () => void;
}

/** Binds a block to live host state: re-reads on mount, on key change and whenever the host signals a change. */
export function useLiveBinding<T>(source: LiveBindingSource<T> | undefined, key: string, fallback: T): T {
  const active = useContext(LiveBindingActivity);
  const previous = useRef(fallback);
  const value = useSyncExternalStore(
    source && active ? source.subscribe : noopSubscribe,
    () => (source && active ? source.read(key) : previous.current),
    () => (source && active ? source.read(key) : previous.current),
  );
  previous.current=value;
  useEffect(() => {
    if(!active) return;
    const release = source?.retain?.(key);
    source?.refresh(key);
    return release;
  }, [source, key, active]);
  return value;
}

function noopSubscribe() {
  return () => {};
}
