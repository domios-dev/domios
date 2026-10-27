import { useEffect, useRef, useState } from "react";
import { listen } from "../../lib/bridge";
import { DEV_BUILD } from "../../lib/devBuild";
import { localFetch } from "../../lib/webApi";
import { SharedRead } from "../../lib/sharedRead";
import { freeLimitGate, upgradePlan } from "../billing/freeLimitGate";
import { accountPlanModel, NoBillingCustomer, SubscriptionServiceUnavailable, type AccountPlan, type PlanModel } from "./accountPlanModel";

const plans = new Map<string | null, SharedRead<PlanModel>>();

async function fetchPlan(focus: boolean): Promise<PlanModel> {
  const response = await localFetch(`/api/account/plan${focus ? "/refresh?focus=true" : ""}`, {method:focus ? "POST" : "GET"});
  if (!response.ok) throw new Error(await response.text());
  return accountPlanModel(await response.json() as AccountPlan);
}

async function openPortal() {
  let response: Response;
  try { response = await localFetch("/api/account/portal",{method:"POST"}); }
  catch (failure: unknown) { throw new SubscriptionServiceUnavailable(failure instanceof Error ? failure.message : String(failure)); }
  if (response.status === 409) {
    const refusal: unknown = await response.json();
    if (refusal && typeof refusal === "object" && "code" in refusal && refusal.code === "no_billing_customer"
      && "message" in refusal && typeof refusal.message === "string") throw new NoBillingCustomer(refusal.message);
    throw new Error("Billing request refused");
  }
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
  const active = useRef(0);
  // A plan loaded for another identity is never shown, even for the render before the reload starts.
  const plan = loaded.key === identityKey ? loaded.plan : null;
  const planLoad: PlanLoad = plan ? "ready" : loaded.key === identityKey && loaded.failed ? "failed" : "loading";
  useEffect(() => {
    const generation = ++active.current;
    const controller = new AbortController();
    if (identityKey === null) for (const key of plans.keys()) { if (key !== null) plans.delete(key); }
    let shared = plans.get(identityKey);
    if (!shared) { shared = new SharedRead<PlanModel>(); plans.set(identityKey, shared); }
    const resource = shared;
    const unsubscribe = resource.subscribe(plan => {
      if (active.current === generation) {
        freeLimitGate.setLimits(plan.limits);
        setLoaded({ key: identityKey, plan, failed: false });
      }
    });
    setLoaded({ key: identityKey, plan: null, failed: false });
    async function refresh(focus = false, force = false): Promise<PlanModel | null> {
      try {
        const current = await resource.read(() => fetchPlan(focus), force);
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
    void refresh();
    const onFocus = () => { void refresh(true); };
    window.addEventListener("focus",onFocus);
    let unlisten: (() => void) | undefined;
    void listen("entitlement-changed", () => { void refresh(false, true); }).then((fn) => {
      if (active.current === generation) unlisten = fn;
      else fn();
    });
    return () => { unsubscribe(); controller.abort(); active.current++; window.removeEventListener("focus",onFocus); unlisten?.(); };
  }, [identityKey]);
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
    manage: () => run(openPortal),
  };
}
