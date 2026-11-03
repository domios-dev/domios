import { useEffect, useState } from "react";
import { listen } from "../../lib/bridge";
import { onResync } from "../../lib/resync";
import type { ApiConfig } from "../../lib/types";
import { fetchReconcile } from "./plansApi";
import type { CommittedPlanChange } from "./planProjection";
import type { DriftItem } from "./types";

export function usePlanDrift(api: ApiConfig | undefined, planIds: string[]): Map<string, DriftItem[]> {
  const key = JSON.stringify([api?.host, api?.port, api?.token, planIds]);
  const [state, setState] = useState<{ key: string; drift: Map<string, DriftItem[]> }>({ key: "", drift: new Map() });
  useEffect(() => {
    if (!api) return;
    let active = true;
    const controllers = new Map<string, AbortController>();
    const stops: Array<() => void> = [];
    const ids: string[] = JSON.parse(key)[3];
    const refresh = (id: string) => {
      if (!active || !ids.includes(id)) return;
      controllers.get(id)?.abort();
      const controller = new AbortController();
      controllers.set(id, controller);
      void fetchReconcile(api, id, controller.signal).catch(() => [] as DriftItem[]).then((drift) => {
        if (!active || controller.signal.aborted) return;
        setState((previous) => {
          const next = new Map(previous.key === key ? previous.drift : []);
          if (drift.length) next.set(id, drift); else next.delete(id);
          return { key, drift: next };
        });
      });
    };
    const reload = () => { for (const id of ids) refresh(id); };
    stops.push(onResync(reload));
    window.addEventListener("codeterm-web-boot-changed", reload);
    stops.push(() => window.removeEventListener("codeterm-web-boot-changed", reload));
    void Promise.all([
      listen<CommittedPlanChange>("plan-changed", ({ payload }) => refresh(payload.plan_id)),
      listen("plans-loaded", reload),
    ].map(async (pending) => { const stop = await pending; if (active) stops.push(stop); else stop(); }))
      .then(() => { if (active) reload(); });
    return () => {
      active = false;
      for (const controller of controllers.values()) controller.abort();
      for (const stop of stops) stop();
    };
  }, [api?.host, api?.port, api?.token, key]);
  return state.key === key ? state.drift : new Map();
}
