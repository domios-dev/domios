import { useCallback, useEffect, useRef, useState } from "react";
import type { NeighborConciergeClient } from "./neighborConciergeClient";
import type { NeighborPermission, NeighborRuntimeClient } from "./neighborRuntimeClient";

/** One friendship's permission, reloaded whenever it changes elsewhere; a late answer never overwrites a newer one. */
export function useNeighborPermission(runtimeClient: NeighborRuntimeClient, conciergeClient: NeighborConciergeClient, friendship: string) {
  const [permission, setPermission] = useState<NeighborPermission | null>(null);
  const generation = useRef(0);
  const reload = useCallback(() => {
    const current = ++generation.current;
    runtimeClient.permission(friendship).then(value => { if (generation.current === current) setPermission(value); }, () => {});
  }, [runtimeClient, friendship]);
  useEffect(() => {
    reload();
    let stop: (() => void) | undefined;
    let live = true;
    void conciergeClient.onPermissionChanged(changed => { if (changed === friendship) reload(); })
      .then(unlisten => { if (live) stop = unlisten; else unlisten(); });
    return () => { live = false; generation.current++; stop?.(); };
  }, [conciergeClient, friendship, reload]);
  return { permission, reload };
}
