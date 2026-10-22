import { useEffect, useRef, useState } from "react";
import { publishNotice } from "../../../lib/notices";
import type { ApiConfig } from "../../../lib/types";
import { fetchPlanControlLoop, setPlanControlLoop, runPlanControlLoop } from "../plansApi";
import type { ControlLoopUpdate, PlanControlLoop } from "../planControlLoop";

export function usePlanControlLoop(api: ApiConfig | undefined, planId: string, revision: number, initial?: PlanControlLoop) {
  const [reading, setReading] = useState<{ planId: string; value: PlanControlLoop } | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const request = useRef(0);
  const pending = useRef(Promise.resolve());
  const queued = useRef(0);
  const identity = JSON.stringify([api?.host, api?.port, api?.token, planId]);
  const active = useRef(identity);
  active.current = identity;
  useEffect(() => {
    const current = ++request.current;
    const controller = new AbortController();
    setLoadError(null);
    if (api) void fetchPlanControlLoop(api, planId, controller.signal).then((value) => {
      if (current === request.current) setReading({ planId, value });
    }).catch((reason: unknown) => {
      if (!controller.signal.aborted && current === request.current) setLoadError(String(reason));
    });
    return () => { controller.abort(); request.current++; };
  }, [api?.host, api?.port, api?.token, planId, revision]);

  useEffect(() => {
    if (initial) setReading({ planId, value: initial });
  }, [planId, initial]);

  const mutate = (perform: () => Promise<PlanControlLoop>) => {
    if (!api) return Promise.resolve();
    queued.current++;
    setBusy(true);
    const run = async () => {
      try {
        if (active.current !== identity) return;
        ++request.current;
        const persisted = await perform();
        if (active.current === identity) {
          setReading({ planId, value: persisted });
          setLoadError(null);
        }
      } catch (reason) {
        publishNotice({ kind: "toast", severity: "Error", id: "plan-settings:" + planId,
          title: "Plan settings update failed", detail: reason instanceof Error ? reason.message : String(reason) });
      } finally {
        queued.current--;
        if (queued.current === 0) setBusy(false);
      }
    };
    pending.current = pending.current.then(run);
    return pending.current;
  };
  const update = (value: ControlLoopUpdate) => api ? mutate(() => setPlanControlLoop(api, planId, value)) : Promise.resolve();
  const runNow = () => api ? mutate(() => runPlanControlLoop(api, planId)) : Promise.resolve();
  return { value: reading?.planId === planId ? reading.value : initial, busy, loadError, update, runNow };
}
