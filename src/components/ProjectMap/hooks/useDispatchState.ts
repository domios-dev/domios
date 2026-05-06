import { useCallback, useEffect, useState } from "react";
import type { ApiConfig } from "../../../lib/types";
import { authHeaders, baseUrl } from "../api";

export interface DispatchState {
  last_dispatch_pane: string | null;
  last_message_template: string | null;
}

const EMPTY: DispatchState = { last_dispatch_pane: null, last_message_template: null };

export function useDispatchState(api: ApiConfig, workspaceId: string | undefined) {
  const [state, setState] = useState<DispatchState>(EMPTY);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    if (!workspaceId) return;
    let cancelled = false;
    const fetchState = async () => {
      try {
        const url = `${baseUrl(api)}/api/map/state?workspace=${encodeURIComponent(workspaceId)}`;
        const res = await fetch(url, { headers: authHeaders(api) });
        if (!res.ok) throw new Error(`${res.status}`);
        const body = await res.json();
        if (!cancelled) {
          setState({
            last_dispatch_pane: body.last_dispatch_pane ?? null,
            last_message_template: body.last_message_template ?? null,
          });
          setLoaded(true);
        }
      } catch {
        if (!cancelled) setLoaded(true);
      }
    };
    fetchState();
    return () => {
      cancelled = true;
    };
  }, [api, workspaceId]);

  const persist = useCallback(
    async (patch: Partial<DispatchState>) => {
      if (!workspaceId) return;
      const next = { ...state, ...patch };
      setState(next);
      try {
        const url = `${baseUrl(api)}/api/map/state?workspace=${encodeURIComponent(workspaceId)}`;
        await fetch(url, {
          method: "PATCH",
          headers: { "Content-Type": "application/json", ...authHeaders(api) },
          body: JSON.stringify(patch),
        });
      } catch {
        // best-effort
      }
    },
    [api, state, workspaceId],
  );

  return { state, loaded, persist };
}
