import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useEffect, useState } from "react";
import { DEV_BUILD } from "../../lib/devBuild";
import { Shield } from "lucide-react";
import { PRODUCT_EVENT_CATALOG, PRODUCT_EVENT_ENUMS } from "../../lib/productAnalytics.generated";
import { localJson } from "../../lib/webApi";
import { Button } from "../shared/Button";
import { SettingsCard, Toggle } from "./shared";
import { analyticsEnabled, changeAnalyticsConsent, type AnalyticsStatus, type PrivacyApi } from "./privacyModel";

const api: PrivacyApi = {
  status: () => localJson("GET", "/api/analytics/status"),
  consent: (consent) => localJson("POST", "/api/analytics/consent", { consent }),
};

export function PrivacyTab() {
  const [status, setStatus] = useState<AnalyticsStatus | null>(null);
  const [loadUnavailable, setLoadUnavailable] = useState(false);
  const [actionError, setActionError] = useState(false);
  const [pending, setPending] = useState(false);
  const [showCollection, setShowCollection] = useState(false);
  useEffect(() => {
    let active = true;
    api.status().then((value) => {
      if (!active) return;
      setStatus(value);
    }).catch((error: unknown) => {
      if (!active) return;
      if (DEV_BUILD) console.debug("Background analytics status refresh failed", error);
      setLoadUnavailable(true);
    });
    return () => { active = false; };
  }, []);

  const change = async (enabled: boolean) => {
    if (pending || !status) return;
    setPending(true);
    setActionError(false);
    try { setStatus(await changeAnalyticsConsent(api, enabled)); }
    catch { setActionError(true); }
    finally { setPending(false); }
  };

  return <SettingsCard icon={<Shield size={16} />} title="Product analytics" description="Share feature usage; prompts, paths, names and terminal output are never collected.">
    <fieldset disabled={pending || !status}><Toggle label="Share product analytics" checked={analyticsEnabled(status)} onChange={(enabled) => { void change(enabled); }} /></fieldset>
    <Button variant="ghost" size="sm" aria-controls="analytics-collection" aria-expanded={showCollection} onClick={() => setShowCollection((open) => !open)}>What we collect</Button>
    {showCollection && <section id="analytics-collection" aria-label="Analytics collection catalog" className="space-y-2 text-xs text-[var(--ct-muted-fg)]">
      <p>With your consent, Domios may send the following product events. Each event accepts only the listed properties and allowed values.</p>
      <ul className="list-disc pl-4">{Object.entries(PRODUCT_EVENT_CATALOG).map(([event, properties]) => <li key={event}><code>{event}</code>: {Object.keys(properties).join(", ") || "no event properties"}</li>)}</ul>
      <details><summary>Allowed property values</summary><ul className="list-disc pl-4">{Object.entries(PRODUCT_EVENT_ENUMS).map(([category, values]) => <li key={category}>{category}: {values.join(", ")}</li>)}</ul></details>
      <p>Properties are limited to numeric app version, platform, machine and workspace counts, registered provider ID, agent role, update kind, authentication provider, plan, subscription status and a closed feature category. Active days and each feature are counted at most once per local calendar day, with those days saved across restarts. Each event carries a random installation ID, event ID and timestamp. While signed in, events also carry your exact Firebase account UID as the Amplitude user ID; signing out removes it from subsequent events.</p>
      <p>We never collect prompts, messages, terminal output, names, paths, email addresses, session IDs or peer identities. Turning analytics off clears queued events and analytics identifiers and stops analytics network traffic. The first-run marker survives opt-out. The installation ID is generated on opt-in and replaced after opting out and back in.</p>
    </section>}
    {loadUnavailable && !status && <p role="status" className="text-xs text-[var(--ct-muted-fg)]">Analytics settings are unavailable.</p>}
    {actionError && <InlineErrorNotice error={{ code: "analytics_settings_unavailable", raw: "" }} />}
  </SettingsCard>;
}
