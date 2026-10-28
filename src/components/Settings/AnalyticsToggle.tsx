import { Toggle } from "./shared";
import { useAnalyticsConsent } from "./useAnalyticsConsent";

export function AnalyticsToggle() {
  const { enabled, loaded, change } = useAnalyticsConsent();
  return <fieldset disabled={!loaded}>
    <Toggle label="Share product analytics" checked={enabled} onChange={value => { void change(value); }} />
    <p className="text-xs text-[var(--ct-muted-fg)]">Share feature usage; prompts, paths, names and terminal output are never collected.</p>
  </fieldset>;
}
