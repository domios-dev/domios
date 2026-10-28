import { useEffect, useRef, useState } from "react";
import { localJson } from "../../lib/webApi";
import { analyticsEnabled, type AnalyticsStatus } from "./privacyModel";

const CONSENT_CHANGED = "codeterm:analytics-consent-changed";
interface ConsentChange { source: symbol; status: AnalyticsStatus | null }
function publishConsent(source: symbol, status: AnalyticsStatus | null) {
  window.dispatchEvent(new CustomEvent<ConsentChange>(CONSENT_CHANGED, { detail: { source, status } }));
}

export function useAnalyticsConsent() {
  const [status, setStatus] = useState<AnalyticsStatus | null>(null);
  const sequence = useRef(0);
  const source = useRef(Symbol("analytics-consent"));
  useEffect(() => {
    let active = true;
    const changed = (event: Event) => {
      const change = (event as CustomEvent<ConsentChange>).detail;
      if (change.source === source.current) return;
      sequence.current++;
      setStatus(change.status);
    };
    window.addEventListener(CONSENT_CHANGED, changed);
    const intent = sequence.current;
    void localJson<AnalyticsStatus>("GET", "/api/analytics/status").then(value => {
      if (active && intent === sequence.current) setStatus(value);
    }).catch(() => {});
    return () => { active = false; window.removeEventListener(CONSENT_CHANGED, changed); };
  }, []);

  const change = async (enabled: boolean) => {
    const intent = ++sequence.current;
    publishConsent(source.current, null);
    setStatus(current => current ? { ...current, consent: enabled ? "on" : "off" } : current);
    try {
      const next = await localJson<AnalyticsStatus>("POST", "/api/analytics/consent", { consent: enabled ? "on" : "off" });
      if (intent === sequence.current) {
        setStatus(next);
        publishConsent(source.current, next);
      }
    } catch {
      try {
        const next = await localJson<AnalyticsStatus>("GET", "/api/analytics/status");
        if (intent === sequence.current) {
          setStatus(next);
          publishConsent(source.current, next);
        }
      } catch {}
    }
  };
  return { enabled: analyticsEnabled(status), loaded: status !== null, change };
}
