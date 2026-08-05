import { useCallback, useEffect, useState } from "react";
import { webJson } from "../../lib/webApi";
import type { CompletedLane } from "./DoneView";

interface DoneResponse {
  completed: CompletedLane[];
}

/** Fetch the durable Done surface from `/api/agents/done`. Refetches when
 *  `revision` changes (e.g. a report just landed) so the list stays live
 *  without polling. */
export function useCompletedLanes(revision: number = 0, limit: number = 200) {
  const [lanes, setLanes] = useState<CompletedLane[]>([]);

  const refresh = useCallback(async () => {
    try {
      const res = await webJson<DoneResponse>("GET", `/api/agents/done?limit=${limit}`);
      setLanes(Array.isArray(res?.completed) ? res.completed : []);
    } catch {
      // A transient fetch failure leaves the last-known list intact.
    }
  }, [limit]);

  useEffect(() => {
    void refresh();
  }, [refresh, revision]);

  return { lanes, refresh };
}
