import { useCallback, useEffect, useRef, useState } from "react";
import { orchestrationTransport, listenOrchestration as listen } from "../../lib/transport/orchestrationRuntime";
import type { CronJob } from "../../lib/types";

export interface CronJobsState {
  jobs: readonly CronJob[] | null;
  failed: boolean;
}

/** Same `cron_list` read as the sidebar, but a failed or malformed read stays unavailable instead of empty; a null workspace reads every Cron on this machine. */
export function useCronJobs(workspaceId: string | null) {
  const [state, setState] = useState<CronJobsState>({ jobs: null, failed: false });
  const sequence = useRef(0);
  const active = useRef(true);

  const refresh = useCallback(async () => {
    const read = ++sequence.current;
    try {
      const next = await orchestrationTransport().cron_list(workspaceId === null ? {} : { workspaceId });
      if (!active.current || read !== sequence.current) return;
      setState(Array.isArray(next) ? { jobs: next, failed: false } : (current) => ({ jobs: current.jobs, failed: true }));
    } catch {
      if (active.current && read === sequence.current) setState((current) => ({ jobs: current.jobs, failed: true }));
    }
  }, [workspaceId]);

  useEffect(() => {
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
  }, [refresh]);

  return { ...state, refresh };
}
