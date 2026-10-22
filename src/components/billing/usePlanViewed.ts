import { useEffect, useRef } from "react";
import { emitProductEvent } from "../../lib/productAnalytics";
import { localJson } from "../../lib/webApi";

async function recordPlanViewed() {
  try {
    const status = await localJson<{ consent: "on" | "off" }>("GET", "/api/analytics/status");
    await emitProductEvent(localJson, status.consent, { event_type: "plan_viewed", event_properties: {} });
  } catch {}
}

export function usePlanViewed() {
  const recorded = useRef(false);
  useEffect(() => {
    if (recorded.current) return;
    recorded.current = true;
    void recordPlanViewed();
  }, []);
}
