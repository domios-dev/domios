import { ShieldCheck } from "lucide-react";
import { SettingsCard, Toggle } from "./shared";
import { useAnalyticsConsent } from "./useAnalyticsConsent";

// The consent switch sits in its own card, like every other General setting, not loose on the page.
export function AnalyticsToggle() {
  const { enabled, loaded, change } = useAnalyticsConsent();
  return <SettingsCard icon={<ShieldCheck size={16} />} title="Privacy"
    description="Share feature usage; prompts, paths, names and terminal output are never collected.">
    <fieldset disabled={!loaded} className="m-0 min-w-0 border-0 p-0">
      <Toggle label="Share product analytics" checked={enabled} onChange={value => { void change(value); }} />
    </fieldset>
  </SettingsCard>;
}
