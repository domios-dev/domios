import { useEffect, useState } from "react";
import type { ApiConfig } from "../../../lib/types";
import { authHeaders, baseUrl } from "../api";

export interface MapProgress {
  running: boolean;
  phase: string;
  done: number;
  total: number;
  started_at_ms: number;
}

const POLL_MS = 800;

export function useMapProgress(api: ApiConfig, workspaceId: string | undefined): MapProgress | null {
  const [progress, setProgress] = useState<MapProgress | null>(null);

  useEffect(() => {
    if (!workspaceId) {
      setProgress(null);
      return;
    }
    let cancelled = false;
    const fetchOnce = async () => {
      try {
        const res = await fetch(
          `${baseUrl(api)}/api/map/progress?workspace=${encodeURIComponent(workspaceId)}`,
          { headers: authHeaders(api) },
        );
        if (!res.ok) return;
        const data = await res.json();
        if (cancelled) return;
        if (data && typeof data === "object" && "running" in data) {
          setProgress(data as MapProgress);
        } else {
          setProgress(null);
        }
      } catch {
        // ignore
      }
    };
    fetchOnce();
    const id = window.setInterval(fetchOnce, POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [api, workspaceId]);

  return progress;
}
