import { useEffect, useRef, useState } from "react";
import type { ApiConfig, CronJob, GraphWorkerInfo } from "../../lib/types";
import { daemonFetch } from "../../lib/webApi";
import { listen } from "../../lib/bridge";
import { authHeaders } from "./memoryApi";
import { dreamHeatmap, type DreamDiaryEntry } from "./dreamModel";
import { isTerminalWorkerStatus } from "../AgentRun/workerEmptyState";
import { dreamRefresh } from "./dreamRefresh";

export function useDreams(api: ApiConfig) {
  const [dreamCron, setDreamCron] = useState<CronJob | null>(null);
  const [dreamWorkers, setDreamWorkers] = useState<GraphWorkerInfo[]>([]);
  const [diaryEntries, setDiaryEntries] = useState<DreamDiaryEntry[]>([]);
  const [diaryLoading, setDiaryLoading] = useState(true);
  const [running, setRunning] = useState(false);
  const [runError, setRunError] = useState<string | null>(null);
  const refresh = useRef<() => Promise<void>>(async () => {});
  const active = useRef(false);
  const starting = useRef(false);

  useEffect(() => {
    active.current = true;
    const abort = new AbortController();
    const isActive = () => !abort.signal.aborted;
    const request = async (path: string) => {
      const res = await daemonFetch(api, path, { headers: authHeaders(api), signal: abort.signal });
      if (!res.ok) throw new Error(`Memory request failed: ${res.status}`);
      return res;
    };
    const fail = (error: unknown) => { if (isActive()) setRunError(String(error)); };
    const refreshWorkers = dreamRefresh(async () => {
      try {
        const [cronResponse, workerResponse] = await Promise.all([request("/api/crons"), request("/api/mem/workers")]);
        const [jobs, workers] = await Promise.all([cronResponse.json() as Promise<CronJob[]>, workerResponse.json() as Promise<GraphWorkerInfo[]>]);
        if (!isActive()) return;
        setDreamCron(jobs.find((job) => job.name === "mem-dreaming-auto") ?? null);
        setDreamWorkers(workers.filter((worker) => worker.intent.startsWith("dreaming:")).sort((a, b) => b.spawned_at - a.spawned_at));
      } catch (error) { fail(error); }
    }, isActive);
    const refreshDiary = dreamRefresh(async () => {
      try {
        const entries: DreamDiaryEntry[] = [];
        const start = dreamHeatmap([], Date.now())[0].at;
        let offset = 0;
        while (isActive()) {
          const params = new URLSearchParams({ limit: "100", offset: String(offset) });
          const res = await request(`/api/mem/dream/diary?${params}`);
          const data = await res.json() as { items: DreamDiaryEntry[]; total: number; next_offset?: number | null };
          entries.push(...data.items);
          const oldest = data.items.at(-1);
          const next = data.next_offset ?? offset + data.items.length;
          if (!oldest || Date.parse(oldest.created_at) < start || next >= data.total || next <= offset) break;
          offset = next;
        }
        if (isActive()) setDiaryEntries(entries);
      } catch (error) { fail(error); }
      finally { if (isActive()) setDiaryLoading(false); }
    }, isActive);
    refresh.current = refreshWorkers;
    setDiaryLoading(true);
    setRunError(null);
    const unlisteners: Array<() => void> = [];
    void (async () => {
      try {
        for (const name of ["graph-worker-spawned", "graph-worker-progress", "graph-worker-reported", "graph-worker-done", "cron-run-finished"]) {
          const unlisten = await listen(name, () => { void refreshWorkers(); void refreshDiary(); });
          if (!isActive()) { unlisten(); return; }
          unlisteners.push(unlisten);
        }
        const unlisten = await listen<{ op: string }>("mem-activity", (event) => {
          if (event.payload?.op !== "read") void refreshDiary();
        });
        if (!isActive()) { unlisten(); return; }
        unlisteners.push(unlisten);
      } catch (error) { fail(error); }
    })();
    void refreshWorkers();
    void refreshDiary();
    return () => {
      active.current = false;
      abort.abort();
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [api.host, api.port, api.token]);

  const mutate = async (path: string, method: string, body?: string) => {
    const res = await daemonFetch(api, path, {
      method, headers: { ...authHeaders(api), "Content-Type": "application/json" }, body,
    });
    if (!res.ok) throw new Error(`Memory request failed: ${res.status} ${await res.text()}`);
    return res;
  };
  const runNow = async () => {
    if (starting.current || dreamWorkers.some((worker) => !isTerminalWorkerStatus(worker.status))) return;
    starting.current = true;
    setRunning(true);
    setRunError(null);
    try {
      await mutate("/api/mem/dream", "POST", JSON.stringify({ phase: "all" }));
      await refresh.current();
    } catch (error) { if (active.current) setRunError(String(error)); }
    finally { starting.current = false; if (active.current) setRunning(false); }
  };
  const toggleDreamCron = async (enable: boolean) => {
    if (!dreamCron) return;
    try {
      const res = await mutate(`/api/crons/${encodeURIComponent(dreamCron.id)}/${enable ? "enable" : "disable"}`, "POST");
      if (active.current) setDreamCron(await res.json() as CronJob);
    } catch (error) { if (active.current) setRunError(String(error)); }
  };
  const stop = async (worker: GraphWorkerInfo) => {
    try {
      await mutate(`/api/mem/workers/${encodeURIComponent(worker.worker_id)}`, "DELETE");
      await refresh.current();
    } catch (error) { if (active.current) setRunError(String(error)); }
  };
  return { dreamCron, dreamWorkers, diaryEntries, diaryLoading, running, runError, runNow, toggleDreamCron, stop };
}

