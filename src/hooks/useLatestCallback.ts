import { useMemo, useRef, type DependencyList } from "react";
import { createLatestCallback, type Callback } from "../lib/latestCallback";

export function useLatestCallback<T extends Callback>(callback: T, dependencies: DependencyList): T {
  const latest = useRef(callback);
  latest.current = callback;
  // The forwarding closure must live outside the caller's shared V8 render context.
  return useMemo(() => createLatestCallback(latest), dependencies);
}
