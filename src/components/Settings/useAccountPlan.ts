import { useEffect, useRef, useState } from "react";
import { DEV_BUILD } from "../../lib/devBuild";
import { localFetch } from "../../lib/webApi";
import { accountPlanModel, type AccountPlan, type PlanModel } from "./accountPlanModel";

export function useAccountPlan(identityKey: string | null) {
  const [plan, setPlan] = useState<PlanModel | null>(null);
  const active = useRef(0);
  useEffect(() => {
    const generation = ++active.current;
    const controller = new AbortController();
    setPlan(null);
    async function refresh(focus = false) {
      try {
        const response = await localFetch(`/api/account/plan${focus ? "/refresh" : ""}`, {method:focus ? "POST" : "GET",signal:controller.signal});
        if (!response.ok) throw new Error(await response.text());
        const value = await response.json() as AccountPlan;
        if (active.current === generation) setPlan(accountPlanModel(value));
      } catch (failure: unknown) {
        if (!controller.signal.aborted && active.current === generation) {
          if (DEV_BUILD) console.debug("Background account plan refresh failed", failure);
        }
      }
    }
    void refresh();
    const onFocus = () => { void refresh(true); };
    window.addEventListener("focus",onFocus);
    return () => { controller.abort(); active.current++; window.removeEventListener("focus",onFocus); };
  }, [identityKey]);
  async function open(action: "upgrade" | "portal", planKey?: string): Promise<unknown | null> {
    try {
      const response = await localFetch(`/api/account/${action}`,{method:"POST",...(action === "upgrade" ? {headers:{"Content-Type":"application/json"},body:JSON.stringify({plan:planKey})} : {})});
      if (!response.ok) throw new Error(await response.text());
      return null;
    } catch (failure: unknown) {
      return failure;
    }
  }
  return {plan,upgrade:(planKey: string) => open("upgrade",planKey),manage:() => open("portal")};
}
