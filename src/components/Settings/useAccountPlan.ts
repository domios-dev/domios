import { useEffect, useRef, useState } from "react";
import { listen } from "../../lib/bridge";
import { DEV_BUILD } from "../../lib/devBuild";
import { localFetch } from "../../lib/webApi";
import { freeLimitGate, upgradePlan } from "../billing/freeLimitGate";
import { accountPlanModel, manageActionForPlan, SubscriptionServiceUnavailable, type AccountPlan, type PlanModel } from "./accountPlanModel";

async function fetchPlan(focus: boolean, signal?: AbortSignal): Promise<PlanModel> {
  const response = await localFetch(`/api/account/plan${focus ? "/refresh" : ""}`, {method:focus ? "POST" : "GET",signal});
  if (!response.ok) throw new Error(await response.text());
  return accountPlanModel(await response.json() as AccountPlan);
}

async function openPortal() {
  let response: Response;
  try { response = await localFetch("/api/account/portal",{method:"POST"}); }
  catch (failure: unknown) { throw new SubscriptionServiceUnavailable(failure instanceof Error ? failure.message : String(failure)); }
  if (response.status === 503) throw new SubscriptionServiceUnavailable(await response.text());
  if (!response.ok) throw new Error(await response.text());
}

export type PlanLoad = "loading" | "ready" | "failed";

interface LoadedPlan {
  key: string | null;
  plan: PlanModel | null;
  failed: boolean;
}

export function useAccountPlan(identityKey: string | null) {
  const [loaded, setLoaded] = useState<LoadedPlan>({ key: identityKey, plan: null, failed: false });
  const [pending, setPending] = useState(false);
  const busy = useRef(false);
  const loading = useRef<Promise<PlanModel | null>>(Promise.resolve(null));
  const active = useRef(0);
  // A plan loaded for another identity is never shown, even for the render before the reload starts.
  const plan = loaded.key === identityKey ? loaded.plan : null;
  const planLoad: PlanLoad = plan ? "ready" : loaded.key === identityKey && loaded.failed ? "failed" : "loading";
  useEffect(() => {
    const generation = ++active.current;
    const controller = new AbortController();
    setLoaded({ key: identityKey, plan: null, failed: false });
    async function refresh(focus = false): Promise<PlanModel | null> {
      try {
        const current = await fetchPlan(focus, controller.signal);
        if (active.current === generation) {
          freeLimitGate.setLimits(current.limits);
          setLoaded({ key: identityKey, plan: current, failed: false });
        }
        return current;
      } catch (failure: unknown) {
        if (!controller.signal.aborted && active.current === generation) {
          if (DEV_BUILD) console.debug("Background account plan refresh failed", failure);
          setLoaded((previous) => previous.plan ? previous : { key: identityKey, plan: null, failed: true });
        }
        return null;
      }
    }
    loading.current = refresh();
    const onFocus = () => { loading.current = refresh(true); };
    window.addEventListener("focus",onFocus);
    let unlisten: (() => void) | undefined;
    void listen("entitlement-changed", () => { loading.current = refresh(); }).then((fn) => {
      if (active.current === generation) unlisten = fn;
      else fn();
    });
    return () => { controller.abort(); active.current++; window.removeEventListener("focus",onFocus); unlisten?.(); };
  }, [identityKey]);
  async function knownPlan(): Promise<PlanModel> {
    const known = plan ?? await loading.current;
    if (known) return known;
    const generation = active.current;
    const fresh = await fetchPlan(false);
    if (active.current === generation) setLoaded({ key: identityKey, plan: fresh, failed: false });
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
    planLoad,
    pending,
    upgrade: (planKey: string) => run(() => upgradePlan(planKey)),
    manage: (openPlans: () => void) => run(async () => {
      if (manageActionForPlan(await knownPlan()) === "portal") await openPortal();
      else openPlans();
    }),
  };
}
