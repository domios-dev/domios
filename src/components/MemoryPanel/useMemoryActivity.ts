import { useEffect, useState } from "react";
import { daemonFetch } from "../../lib/webApi";
import { listen } from "../../lib/bridge";
import type { ApiConfig, CronJob } from "../../lib/types";
import { authHeaders } from "./memoryApi";
import type { MemActivityEvent } from "./useActivityPulses";

export interface ActivityResource<T> {
  data: T | null;
  loading: boolean;
  error: string | null;
}

export interface MemorySyncStatus {
  tracked_files: number;
  stale_files: number;
}

function useActivityResource<T>(
  api: ApiConfig,
  path: string,
  revision: number,
): ActivityResource<T> {
  const [state, setState] = useState<ActivityResource<T>>({
    data: null,
    loading: true,
    error: null,
  });
  const { host, port, token } = api;
  useEffect(() => {
    let active = true;
    const connection = { host, port, token } as ApiConfig;
    setState({ data: null, loading: true, error: null });
    daemonFetch(connection, path, { headers: authHeaders(connection) })
      .then(async (response) => {
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        return (await response.json()) as T;
      })
      .then((data) => {
        if (active) setState({ data, loading: false, error: null });
      })
      .catch((error: unknown) => {
        if (active) setState({ data: null, loading: false, error: String(error) });
      });
    return () => {
      active = false;
    };
  }, [host, port, token, path, revision]);
  return state;
}

export function useMemoryActivity(api: ApiConfig, revision: number) {
  const sync = useActivityResource<MemorySyncStatus>(api, "/api/mem/sync/status", revision);
  const crons = useActivityResource<CronJob[]>(api, "/api/crons", revision);
  const recent = useActivityResource<{ events: MemActivityEvent[] }>(
    api,
    "/api/mem/activity/recent?since_ms=0",
    revision,
  );
  const [live, setLive] = useState<MemActivityEvent[]>([]);
  const [streamError, setStreamError] = useState<string | null>(null);
  const { host, port, token } = api;
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    setLive([]);
    setStreamError(null);
    listen<MemActivityEvent>("mem-activity", (event) => {
      if (active) setLive((previous) => [event.payload, ...previous].slice(0, 500));
    })
      .then((stop) => {
        if (active) unlisten = stop;
        else stop();
      })
      .catch((error: unknown) => {
        if (active) setStreamError(String(error));
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [host, port, token, revision]);
  const unique = new Map<string, MemActivityEvent>();
  for (const event of [...live, ...(recent.data?.events ?? [])]) {
    unique.set(JSON.stringify([event.ts_ms, event.op, event.source, event.mem_ids]), event);
  }
  const events = [...unique.values()].sort((a, b) => b.ts_ms - a.ts_ms).slice(0, 500);
  return { sync, crons, events, recent, streamError };
}
