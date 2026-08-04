import { useEffect, useState } from "react";
import { invoke, listen } from "../lib/bridge";
import type { RemoteWorkersByOrchestrator } from "../lib/types";

/** Live map of orchestrator tab id → remote (cross-mesh) workers. Seeds from
 *  `get_remote_workers`, then applies `remote-workers-changed` event payloads
 *  (each carries the full snapshot). Empty until the first remote worker
 *  reports, so local-only setups pay nothing. */
export function useRemoteWorkers(): RemoteWorkersByOrchestrator {
  const [workers, setWorkers] = useState<RemoteWorkersByOrchestrator>({});

  useEffect(() => {
    let cancelled = false;
    invoke<RemoteWorkersByOrchestrator>("get_remote_workers")
      .then((map) => {
        if (!cancelled && map) setWorkers(map);
      })
      .catch(() => {});

    const unlistenP = listen<{ workers?: RemoteWorkersByOrchestrator }>(
      "remote-workers-changed",
      (e) => {
        if (e.payload?.workers) setWorkers(e.payload.workers);
      },
    );

    return () => {
      cancelled = true;
      unlistenP.then((u) => u()).catch(() => {});
    };
  }, []);

  return workers;
}
