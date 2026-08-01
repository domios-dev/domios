import { useCallback, useEffect, useState } from "react";
import { invoke, listen } from "../lib/bridge";
import type { CronJob } from "../lib/types";

export function useWorkspaceCrons(workspaceId: string) {
  const [jobs, setJobs] = useState<CronJob[]>([]);

  const refresh = useCallback(async () => {
    const next = await invoke<CronJob[]>("cron_list", { workspaceId });
    setJobs(Array.isArray(next) ? next : []);
  }, [workspaceId]);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void refresh().catch(() => {
      if (active) setJobs([]);
    });
    void listen("cron-run-finished", () => void refresh()).then((stop) => {
      if (active) unlisten = stop;
      else stop();
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [refresh]);

  return { jobs, refresh };
}
