import { PageDataContext } from "./PageDataContext";
import { useCallback, useEffect, useMemo, useState, useContext, useRef } from "react";
import { invoke, listen } from "../lib/bridge";
import type { CronJob } from "../lib/types";

export function useWorkspaceCrons(workspaceId: string) {
  const page = useContext(PageDataContext);
  const local = useWorkspaceCronsLoader(workspaceId, !page);
  const pageJobs = page?.crons.jobs;
  const jobs = useMemo(() => pageJobs?.filter((job) => job.workspace_id === workspaceId), [pageJobs, workspaceId]);
  if (page && jobs) return { jobs, refresh: page.crons.refresh };
  return local;
}

export function useWorkspaceCronsLoader(workspaceId?: string, enabled = true) {
  const sequence = useRef(0);
  const [loading, setLoading] = useState(true);
  const [ready, setReady] = useState(false);
  const [failed, setFailed] = useState(false);
  const [jobs, setJobs] = useState<CronJob[]>([]);

  const refresh = useCallback(async () => {
    const request = ++sequence.current;
    try {
      const next = await invoke<CronJob[]>("cron_list", workspaceId ? { workspaceId } : {});
      if (request !== sequence.current) return;
      if (!Array.isArray(next)) throw new Error("Cron list is unavailable");
      setJobs(next);
      setReady(true);
      setFailed(false);
    } catch {
      if (request === sequence.current) setFailed(true);
    } finally {
      if (request === sequence.current) setLoading(false);
    }
  }, [workspaceId]);

  useEffect(() => {
    if (!enabled) return;
    let active = true;
    let unlisten: (() => void) | undefined;
    void refresh().catch(() => {
      if (active) setJobs([]);
    });
    void listen("cron-run-finished", () => { void refresh().catch(() => undefined); }).then((stop) => {
      if (active) unlisten = stop;
      else stop();
    }).catch(() => undefined);
    const reset = () => { sequence.current++; setJobs([]); setLoading(true); setReady(false); setFailed(false); };
    const rebind = () => { void refresh().catch(() => undefined); };
    window.addEventListener("codeterm-peer-switch-reset", reset);
    window.addEventListener("codeterm-peer-switch-rebind", rebind);
    return () => {
      sequence.current++;
      window.removeEventListener("codeterm-peer-switch-reset", reset);
      window.removeEventListener("codeterm-peer-switch-rebind", rebind);
      active = false;
      unlisten?.();
    };
  }, [refresh, enabled]);

  return { jobs, refresh, loading, ready, failed };
}
