import { useEffect, useState } from "react";
import type { ApiConfig } from "../../../lib/types";
import { authHeaders, baseUrl } from "../api";

export interface DispatchablePane {
  pane_id: string;
  title: string;
  role: string;
  agent_type: string | null;
}

export function useDispatchablePanes(api: ApiConfig, workspaceId: string | undefined) {
  const [panes, setPanes] = useState<DispatchablePane[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!workspaceId) {
      setPanes([]);
      return;
    }
    let cancelled = false;
    const refresh = async () => {
      try {
        const url = `${baseUrl(api)}/api/map/dispatchable?workspace=${encodeURIComponent(workspaceId)}`;
        const res = await fetch(url, { headers: authHeaders(api) });
        if (!res.ok) throw new Error(`${res.status}: ${await res.text()}`);
        const list: DispatchablePane[] = await res.json();
        if (!cancelled) {
          setPanes(list);
          setError(null);
        }
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      }
    };
    refresh();
    const t = window.setInterval(refresh, 4000);
    return () => {
      cancelled = true;
      window.clearInterval(t);
    };
  }, [api, workspaceId]);

  return { panes, error };
}
