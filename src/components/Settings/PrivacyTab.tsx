import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useEffect, useState } from "react";
import { Shield } from "lucide-react";
import { PRODUCT_EVENT_CATALOG, PRODUCT_EVENT_ENUMS } from "../../lib/productAnalytics.generated";
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
  const [showCollection, setShowCollection] = useState(false);
  useEffect(() => {
    let active = true;
    api.status().then((value) => {
      if (!active) return;
      setStatus(value);
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
    <a href="#analytics-collection" onClick={() => setShowCollection(true)}>What we collect</a>
    {showCollection && <section id="analytics-collection" aria-label="Analytics collection catalog">
      <p>With your consent, Domios may send the following product events. Each event accepts only the listed properties and allowed values.</p>
      <ul>{Object.entries(PRODUCT_EVENT_CATALOG).map(([event, properties]) => <li key={event}><code>{event}</code>: {Object.keys(properties).join(", ") || "no event properties"}</li>)}</ul>
      <details><summary>Allowed property values</summary><ul>{Object.entries(PRODUCT_EVENT_ENUMS).map(([category, values]) => <li key={category}>{category}: {values.join(", ")}</li>)}</ul></details>
      <p>Properties are limited to numeric app version, platform, machine and workspace counts, registered provider ID, agent role, update kind, authentication provider, plan, subscription status and a closed feature category. Active days and each feature are counted at most once per local calendar day, with those days saved across restarts. Each event carries a random installation ID, event ID and timestamp. While signed in, events also carry your exact Firebase account UID as the Amplitude user ID; signing out removes it from subsequent events.</p>
      <p>We never collect prompts, messages, terminal output, names, paths, email addresses, session IDs or peer identities. Turning analytics off clears queued events and analytics identifiers and stops analytics network traffic. The installation ID is generated on opt-in and replaced after opting out and back in.</p>
    </section>}
    {error && <InlineErrorNotice error={{ code: "analytics_settings_unavailable", raw: "" }} />}
  </SettingsCard>;
}
