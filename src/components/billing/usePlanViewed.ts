import { useEffect, useRef } from "react";
import { emitProductEvent } from "../../lib/productAnalytics";
import { localJson } from "../../lib/webApi";

async function recordPlanViewed() {
  try {
    const status = await localJson<{ consent: "on" | "off" }>("GET", "/api/analytics/status");
    await emitProductEvent(localJson, status.consent, { event_type: "plan_viewed", event_properties: {} });
  } catch {}
}

export function usePlanViewed(requested: boolean) {
  const recorded = useRef(false);
  const target = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const record = () => {
      if (recorded.current) return;
      recorded.current = true;
      void recordPlanViewed();
    };
    if (requested) record();
    const element = target.current;
    if (!element || typeof IntersectionObserver === "undefined" || recorded.current) return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some(entry => entry.isIntersecting)) {
        record();
        observer.disconnect();
      }
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [requested]);
  return target;
}
