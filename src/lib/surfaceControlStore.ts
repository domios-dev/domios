import { useCallback, useEffect, useState } from "react";
import { createKeyedFlagStore, useKeyedFlag } from "./keyedFlagStore";

/** Which surface tabs the user has taken control of, keyed by surface tab id. */
export const surfaceControlStore = createKeyedFlagStore();

export function useSurfaceControlled(surfaceTabId: string | null | undefined): boolean {
  return useKeyedFlag(surfaceControlStore, surfaceTabId);
}

type Update = boolean | ((current: boolean) => boolean);

/** Take-control state: shared through the store when keyed, local otherwise. */
export function useSurfaceTakeControl(surfaceTabId: string | null | undefined): [boolean, (next: Update) => void] {
  const [local, setLocal] = useState(false);
  const shared = useSurfaceControlled(surfaceTabId);
  const value = surfaceTabId ? shared : local;
  const set = useCallback((next: Update) => {
    if (!surfaceTabId) {
      setLocal(next);
      return;
    }
    const current = surfaceControlStore.get(surfaceTabId);
    surfaceControlStore.set(surfaceTabId, typeof next === "function" ? next(current) : next);
  }, [surfaceTabId]);
  // An unmounted view holds no input, so control must not outlive it.
  useEffect(() => {
    if (!surfaceTabId) return;
    return () => surfaceControlStore.set(surfaceTabId, false);
  }, [surfaceTabId]);
  return [value, set];
}
