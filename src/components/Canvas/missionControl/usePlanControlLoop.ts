import { useEffect, useRef, useState } from "react";
import type { ApiConfig } from "../../../lib/types";
import { fetchPlanControlLoop, setPlanControlLoop } from "../plansApi";
import type { ControlLoopUpdate, PlanControlLoop } from "../planControlLoop";

export function usePlanControlLoop(api: ApiConfig | undefined, planId: string, revision: number, initial?: PlanControlLoop) {
  const [reading, setReading] = useState<{ planId: string; value: PlanControlLoop } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const request = useRef(0);
  const writing = useRef(false);
  useEffect(() => {
    const current = ++request.current;
    const controller = new AbortController();
    setError(null);
    if (api) void fetchPlanControlLoop(api, planId, controller.signal).then((value) => {
      if (current === request.current) setReading({ planId, value });
    }).catch((reason: unknown) => {
      if (!controller.signal.aborted && current === request.current) setError(String(reason));
    });
    return () => { controller.abort(); request.current++; };
  }, [api?.host, api?.port, api?.token, planId, revision]);

  useEffect(() => {
    if (initial) setReading({ planId, value: initial });
  }, [planId, initial]);

  const update = async (value: ControlLoopUpdate) => {
    if (!api || writing.current) return;
    writing.current = true;
    setBusy(true);
    setError(null);
    const current = ++request.current;
    try {
      await setPlanControlLoop(api, planId, value);
      const persisted = await fetchPlanControlLoop(api, planId);
      if (current === request.current) setReading({ planId, value: persisted });
    } catch (reason) {
      if (current === request.current) setError(String(reason));
    } finally {
      writing.current = false;
      setBusy(false);
    }
  };
  return { value: reading?.planId === planId ? reading.value : initial, busy, error, update };
}
