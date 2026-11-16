import { ShieldCheck } from "lucide-react";
import { SettingsCard, Toggle } from "./shared";
import { useAnalyticsConsent } from "./useAnalyticsConsent";
import { PRODUCT_ANALYTICS_PRIVACY_SUMMARY } from "../../lib/productAnalytics.generated";

// The consent switch sits in its own card, like every other General setting, not loose on the page.
export function AnalyticsToggle() {
  const { enabled, loaded, change } = useAnalyticsConsent();
  return <SettingsCard icon={<ShieldCheck size={16} />} title="Privacy"
    description={PRODUCT_ANALYTICS_PRIVACY_SUMMARY}>
    <fieldset disabled={!loaded} className="m-0 min-w-0 border-0 p-0">
      <Toggle label="Share product analytics" checked={enabled} onChange={value => { void change(value); }} />
    </fieldset>
  </SettingsCard>;
}
