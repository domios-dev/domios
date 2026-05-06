import { useCallback, useEffect, useRef, useState } from "react";
import type { ApiConfig } from "../../../lib/types";
import { fetchProjectMapGraph } from "../api";
import type { ProjectMapPayload, ProjectMapStats } from "../types";

interface State {
  payload: ProjectMapPayload | null;
  stats: ProjectMapStats | null;
  truncated: boolean;
  error: string | null;
  loading: boolean;
}

const INITIAL: State = {
  payload: null,
  stats: null,
  truncated: false,
  error: null,
  loading: false,
};

export function useProjectMapGraph(api: ApiConfig, workspaceId: string | undefined) {
  const [state, setState] = useState<State>(INITIAL);
  const tokenRef = useRef(0);

  const refetch = useCallback(() => {
    if (!workspaceId) {
      setState(INITIAL);
      return;
    }
    const token = ++tokenRef.current;
    const ctrl = new AbortController();
    setState((prev) => ({ ...prev, loading: true, error: null }));
    fetchProjectMapGraph(api, workspaceId, ctrl.signal)
      .then((payload) => {
        if (token !== tokenRef.current) return;
        setState({
          payload,
          stats: payload.stats ?? null,
          truncated: !!payload.truncated,
          error: null,
          loading: false,
        });
      })
      .catch((err) => {
        if (ctrl.signal.aborted) return;
        if (token !== tokenRef.current) return;
        setState({
          payload: null,
          stats: null,
          truncated: false,
          error: err instanceof Error ? err.message : String(err),
          loading: false,
        });
      });
    return () => {
      ctrl.abort();
    };
  }, [api, workspaceId]);

  useEffect(() => {
    const teardown = refetch();
    return () => {
      if (typeof teardown === "function") teardown();
    };
  }, [refetch]);

  return {
    payload: state.payload,
    stats: state.stats,
    truncated: state.truncated,
    error: state.error,
    loading: state.loading,
    refetch,
  };
}
