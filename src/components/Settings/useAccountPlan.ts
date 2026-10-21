import { useEffect, useRef, useState } from "react";
import { DEV_BUILD } from "../../lib/devBuild";
import { localFetch } from "../../lib/webApi";
import { freeLimitGate } from "../billing/freeLimitGate";
import { accountPlanModel, manageActionForPlan, type AccountPlan, type PlanModel } from "./accountPlanModel";

async function fetchPlan(focus: boolean, signal?: AbortSignal): Promise<PlanModel> {
  const response = await localFetch(`/api/account/plan${focus ? "/refresh" : ""}`, {method:focus ? "POST" : "GET",signal});
  if (!response.ok) throw new Error(await response.text());
  return accountPlanModel(await response.json() as AccountPlan);
}

async function openBilling(action: "upgrade" | "portal", planKey?: string) {
  const response = await localFetch(`/api/account/${action}`,{method:"POST",...(action === "upgrade" ? {headers:{"Content-Type":"application/json"},body:JSON.stringify({plan:planKey})} : {})});
  if (!response.ok) throw new Error(await response.text());
}

export function useAccountPlan(identityKey: string | null) {
  const [plan, setPlan] = useState<PlanModel | null>(null);
  const [pending, setPending] = useState(false);
  const busy = useRef(false);
  const loading = useRef<Promise<PlanModel | null>>(Promise.resolve(null));
  const active = useRef(0);
  useEffect(() => {
    const generation = ++active.current;
    const controller = new AbortController();
    setPlan(null);
    async function refresh(focus = false): Promise<PlanModel | null> {
      try {
        const current = await fetchPlan(focus, controller.signal);
        if (active.current === generation) {
          freeLimitGate.setLimits(current.limits);
          setPlan(current);
        }
        return current;
      } catch (failure: unknown) {
        if (!controller.signal.aborted && active.current === generation) {
          if (DEV_BUILD) console.debug("Background account plan refresh failed", failure);
        }
        return null;
      }
    }
    loading.current = refresh();
    const onFocus = () => { loading.current = refresh(true); };
    window.addEventListener("focus",onFocus);
    return () => { controller.abort(); active.current++; window.removeEventListener("focus",onFocus); };
  }, [identityKey]);
  async function knownPlan(): Promise<PlanModel> {
    const known = plan ?? await loading.current;
    if (known) return known;
    const generation = active.current;
    const fresh = await fetchPlan(false);
    if (active.current === generation) setPlan(fresh);
    return fresh;
  }
  async function run(action: () => Promise<void>): Promise<unknown | null> {
    if (busy.current) return null;
    busy.current = true;
    setPending(true);
    try {
      await action();
      return null;
    } catch (failure: unknown) {
      return failure;
    } finally {
      busy.current = false;
      setPending(false);
    }
  }
  return {
    plan,
    pending,
    upgrade: (planKey: string) => run(() => openBilling("upgrade", planKey)),
    manage: (openPlans: () => void) => run(async () => {
      if (manageActionForPlan(await knownPlan()) === "portal") await openBilling("portal");
      else openPlans();
    }),
  };
}
