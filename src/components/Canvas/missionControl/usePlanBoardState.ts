import { useEffect, useState } from "react";
import type { ApiConfig } from "../../../lib/types";
import { fetchPlanBoardState, type PlanBoardState } from "./missionControlState";

export interface PlanBoardStateResult {
  state: PlanBoardState | null;
  loading: boolean;
  error: string | null;
}

/** One stored snapshot per plan, refetched when the plan record itself changes. */
export function usePlanBoardState(
  api: ApiConfig | undefined,
  planId: string | null,
  revision: number,
): PlanBoardStateResult {
  const [state, setState] = useState<PlanBoardState | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!api || !planId) {
      setState(null);
      setError(null);
      setLoading(false);
      return;
    }
    const controller = new AbortController();
    setLoading(true);
    fetchPlanBoardState(api, planId, controller.signal)
      .then((next) => {
        if (controller.signal.aborted) return;
        setState(next);
        setError(null);
      })
      .catch((e: unknown) => {
        if (controller.signal.aborted) return;
        setState(null);
        setError(`Mission Control state unavailable: ${String(e)}`);
      })
      .finally(() => {
        if (!controller.signal.aborted) setLoading(false);
      });
    return () => controller.abort();
  }, [api, planId, revision]);

  return { state, loading, error };
}
