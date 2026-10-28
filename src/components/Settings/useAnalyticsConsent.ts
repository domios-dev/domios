import { useEffect, useRef, useState } from "react";
import { localJson } from "../../lib/webApi";
import { analyticsEnabled, type AnalyticsStatus } from "./privacyModel";

export function useAnalyticsConsent() {
  const [status, setStatus] = useState<AnalyticsStatus | null>(null);
  const sequence = useRef(0);
  useEffect(() => {
    let active = true;
    const intent = sequence.current;
    void localJson<AnalyticsStatus>("GET", "/api/analytics/status").then(value => {
      if (active && intent === sequence.current) setStatus(value);
    }).catch(() => {});
    return () => { active = false; };
  }, []);

  const change = async (enabled: boolean) => {
    const intent = ++sequence.current;
    setStatus(current => current ? { ...current, consent: enabled ? "on" : "off" } : current);
    try {
      const next = await localJson<AnalyticsStatus>("POST", "/api/analytics/consent", { consent: enabled ? "on" : "off" });
      if (intent === sequence.current) setStatus(next);
    } catch {
      try {
        const next = await localJson<AnalyticsStatus>("GET", "/api/analytics/status");
        if (intent === sequence.current) setStatus(next);
      } catch {}
    }
  };
  return { enabled: analyticsEnabled(status), loaded: status !== null, change };
}
