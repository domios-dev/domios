import { ShieldCheck } from "lucide-react";
import { RemoteLocalOnlyNotice, SettingsCard, Toggle } from "./shared";
import { useAnalyticsConsent } from "./useAnalyticsConsent";
import { useSettingsRemoteTarget } from "./useSettingsTarget";
import { PRODUCT_ANALYTICS_PRIVACY_SUMMARY } from "../../lib/productAnalytics.generated";

// The consent switch sits in its own card, like every other General setting, not loose on the page.
export function AnalyticsToggle() {
  const remoteTarget = useSettingsRemoteTarget();
  const { enabled, loaded, change } = useAnalyticsConsent(!remoteTarget);
  return <SettingsCard icon={<ShieldCheck size={16} />} title="Privacy"
    description={PRODUCT_ANALYTICS_PRIVACY_SUMMARY}>
    {remoteTarget ? (
      <>
        <RemoteLocalOnlyNotice machineName={remoteTarget.name} subject="Product analytics" />
        <Toggle label="Share product analytics" checked={false} disabled onChange={() => {}} />
      </>
    ) : (
      <fieldset disabled={!loaded} className="m-0 min-w-0 border-0 p-0">
        <Toggle label="Share product analytics" checked={enabled} onChange={value => { void change(value); }} />
      </fieldset>
    )}
  </SettingsCard>;
}
