import { PageDataContext } from "../../hooks/PageDataContext";
import { useCallback, useEffect, useRef, useState, useContext } from "react";
import { orchestrationTransport, listenOrchestration as listen } from "../../lib/transport/orchestrationRuntime";
import { DEV_BUILD } from "../../lib/devBuild";
import type { CronJob } from "../../lib/types";

export interface CronJobsState {
  jobs: readonly CronJob[] | null;
  failed: boolean;
}

/** Same `cron_list` read as the sidebar, but a failed or malformed read stays unavailable instead of empty; a null workspace reads every Cron on this machine. */
export function useCronJobs(workspaceId: string | null) {
  const page = useContext(PageDataContext);
  const local = useCronJobsLoader(workspaceId, !page);
  if (!page) return local;
  const { jobs, refresh, ready, failed } = page.crons;
  return { jobs: !ready ? null : jobs.filter((job) => workspaceId === null || job.workspace_id === workspaceId), failed, refresh };
}

function useCronJobsLoader(workspaceId: string | null, enabled: boolean) {
  const [state, setState] = useState<CronJobsState>({ jobs: null, failed: false });
  const sequence = useRef(0);
  const active = useRef(true);

  const refresh = useCallback(async () => {
    const read = ++sequence.current;
    try {
      const next = await orchestrationTransport().cron_list(workspaceId === null ? {} : { workspaceId });
      if (!active.current || read !== sequence.current) return;
      if (Array.isArray(next)) setState({ jobs: next, failed: false });
      else {
        if (DEV_BUILD) console.debug("Background Cron list refresh returned invalid data");
        setState((current) => ({ jobs: current.jobs, failed: true }));
      }
    } catch (error) {
      if (!active.current || read !== sequence.current) return;
      if (DEV_BUILD) console.debug("Background Cron list refresh failed", error);
      setState((current) => ({ jobs: current.jobs, failed: true }));
    }
  }, [workspaceId]);

  useEffect(() => {
    if (!enabled) return;
    active.current = true;
    let unlisten: (() => void) | undefined;
    void refresh();
    void listen("cron-run-finished", () => { if (active.current) void refresh(); }).then((stop) => {
      if (active.current) unlisten = stop;
      else stop();
    }, () => undefined);
    return () => {
      active.current = false;
      unlisten?.();
    };
  }, [refresh, enabled]);

  return { ...state, refresh };
}
