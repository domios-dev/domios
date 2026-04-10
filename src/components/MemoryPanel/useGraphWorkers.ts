import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type {
  ApiConfig,
  GraphWorkerInfo,
  SpawnWorkerReq,
} from "../../lib/types";
import { baseUrl, authHeaders, jsonHeaders, isTauriEnv } from "./memoryApi";

const POLL_INTERVAL_MS = 3000;

export interface UseGraphWorkersResult {
  workers: GraphWorkerInfo[];
  loading: boolean;
  error: string | null;
  spawn: (req: SpawnWorkerReq) => Promise<GraphWorkerInfo>;
  kill: (workerId: string) => Promise<void>;
  refresh: () => Promise<void>;
}

export function useGraphWorkers(api: ApiConfig): UseGraphWorkersResult {
  const [workers, setWorkers] = useState<GraphWorkerInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const activeRef = useRef(true);

  const upsert = useCallback((info: GraphWorkerInfo) => {
    setWorkers((prev) => {
      const idx = prev.findIndex((w) => w.worker_id === info.worker_id);
      if (idx === -1) return [info, ...prev];
      const next = prev.slice();
      next[idx] = info;
      return next;
    });
  }, []);

  const refresh = useCallback(async () => {
    try {
      const res = await fetch(`${baseUrl(api)}/api/mem/workers`, {
        headers: authHeaders(api),
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = (await res.json()) as GraphWorkerInfo[];
      if (!activeRef.current) return;
      data.sort((a, b) => b.spawned_at - a.spawned_at);
      setWorkers(data);
      setError(null);
    } catch (e) {
      if (!activeRef.current) return;
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (activeRef.current) setLoading(false);
    }
  }, [api]);

  useEffect(() => {
    activeRef.current = true;
    let unlisteners: Array<() => void> = [];
    let pollTimer: ReturnType<typeof setInterval> | undefined;

    refresh();

    const setup = async () => {
      if (isTauriEnv()) {
        try {
          const names = [
            "graph-worker-spawned",
            "graph-worker-progress",
            "graph-worker-reported",
            "graph-worker-done",
          ];
          for (const name of names) {
            const fn = await listen<GraphWorkerInfo>(name, (ev) => {
              if (!activeRef.current) return;
              upsert(ev.payload);
            });
            if (!activeRef.current) {
              fn();
              return;
            }
            unlisteners.push(fn);
          }
        } catch {
          startPolling();
        }
      } else {
        startPolling();
      }
    };

    const startPolling = () => {
      if (pollTimer != null) return;
      pollTimer = setInterval(() => {
        if (!activeRef.current) return;
        refresh();
      }, POLL_INTERVAL_MS);
    };

    setup();

    return () => {
      activeRef.current = false;
      for (const u of unlisteners) u();
      unlisteners = [];
      if (pollTimer != null) clearInterval(pollTimer);
    };
  }, [api, refresh, upsert]);

  const spawn = useCallback(
    async (req: SpawnWorkerReq): Promise<GraphWorkerInfo> => {
      const res = await fetch(`${baseUrl(api)}/api/mem/workers`, {
        method: "POST",
        headers: jsonHeaders(api),
        body: JSON.stringify(req),
      });
      if (!res.ok) {
        const txt = await res.text().catch(() => "");
        throw new Error(`Spawn failed: ${res.status} ${txt}`);
      }
      const data = (await res.json()) as {
        worker_id: string;
        pane_id: string;
        info: GraphWorkerInfo;
      };
      upsert(data.info);
      return data.info;
    },
    [api, upsert],
  );

  const kill = useCallback(
    async (workerId: string): Promise<void> => {
      const res = await fetch(`${baseUrl(api)}/api/mem/workers/${workerId}`, {
        method: "DELETE",
        headers: authHeaders(api),
      });
      if (!res.ok) {
        throw new Error(`Kill failed: ${res.status}`);
      }
    },
    [api],
  );

  return { workers, loading, error, spawn, kill, refresh };
}
