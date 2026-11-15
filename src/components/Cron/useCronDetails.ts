import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, listen } from "../../lib/bridge";
import { DEV_BUILD } from "../../lib/devBuild";
import { readWhenMuxCapacityAvailable } from "../../lib/streamMuxRequestModel";
import { orchestrationTransport } from "../../lib/transport/orchestrationRuntime";
import type { CronJob } from "../../lib/types";
import type { MeshInventoryView } from "../MeshPanel/servicesView";
import { CRON_ID_PREFIX, type Service } from "../MeshPanel/types";

export function selectCronService(inventory: MeshInventoryView, cronId: string): Service | null {
  return inventory.resources.find((resource) => resource.id === `${CRON_ID_PREFIX}${cronId}`) ?? null;
}

interface CronDetails {
  cronId: string;
  job: CronJob | null;
  service: Service | null;
  error: unknown;
}

export function useCronDetails(cronId: string) {
  const [state, setState] = useState<CronDetails>({ cronId, job: null, service: null, error: null });
  const refreshRef = useRef<(() => Promise<void>) | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    let off: (() => void) | undefined;
    let reading = false;
    let dirty = false;
    const refresh = async () => {
      if (reading) { dirty = true; return; }
      reading = true;
      try {
        do {
          dirty = false;
          try {
            const [job, inventory] = await Promise.all([
              readWhenMuxCapacityAvailable(() => orchestrationTransport().cron_get({ id: cronId }), controller.signal),
              readWhenMuxCapacityAvailable(() => invoke<MeshInventoryView>("mesh_inventory_list"), controller.signal),
            ]);
            if (!controller.signal.aborted) setState({ cronId, job, service: selectCronService(inventory, cronId), error: null });
          } catch (error) {
            if (controller.signal.aborted) return;
            if (DEV_BUILD) console.debug("Background Cron details refresh failed", error);
            setState((current) => current.cronId === cronId && current.job ? current : { cronId, job: null, service: null, error });
          }
        } while (dirty && !controller.signal.aborted);
      } finally { reading = false; }
    };
    refreshRef.current = refresh;
    void refresh();
    void listen("cron-run-finished", () => { if (!controller.signal.aborted) void refresh(); }).then((stop) => {
      if (controller.signal.aborted) stop();
      else off = stop;
    });
    return () => { controller.abort(); off?.(); if (refreshRef.current === refresh) refreshRef.current = null; };
  }, [cronId]);
  const refresh = useCallback(() => refreshRef.current?.() ?? Promise.resolve(), []);
  const current = state.cronId === cronId ? state : { cronId, job: null, service: null, error: null };
  return { ...current, refresh };
}
