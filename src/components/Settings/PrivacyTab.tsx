import { useEffect, useState } from "react";
import { Shield } from "lucide-react";
import { localJson } from "../../lib/webApi";
import { SettingsCard, Toggle } from "./shared";
import { analyticsEnabled, changeAnalyticsConsent, type AnalyticsStatus, type PrivacyApi } from "./privacyModel";

const api: PrivacyApi = {
  status: () => localJson("GET", "/api/analytics/status"),
  consent: (consent) => localJson("POST", "/api/analytics/consent", { consent }),
};

export function PrivacyTab() {
  const [status, setStatus] = useState<AnalyticsStatus | null>(null);
  const [error, setError] = useState(false);
  const [pending, setPending] = useState(false);
  useEffect(() => {
    let active = true;
    api.status().then((value) => {
      if (!active) return;
      setStatus(value);
      if (value.consent === "on") {
        void localJson("POST", "/api/analytics/event", { event_type: "settings_opened", event_properties: { section: "privacy" } }).catch(() => {});
      }
    }).catch(() => { if (active) setError(true); });
    return () => { active = false; };
  }, []);

  const change = async (enabled: boolean) => {
    if (pending || !status) return;
    setPending(true);
    setError(false);
    try { setStatus(await changeAnalyticsConsent(api, enabled)); }
    catch { setError(true); }
    finally { setPending(false); }
  };

  return <SettingsCard icon={<Shield size={16} />} title="Product analytics" description="Share feature usage; prompts, paths, names and terminal output are never collected.">
    <fieldset disabled={pending || !status}><Toggle label="Share product analytics" checked={analyticsEnabled(status)} onChange={(enabled) => { void change(enabled); }} /></fieldset>
    {error && <p role="alert">Analytics settings could not be saved or loaded.</p>}
  </SettingsCard>;
}
