import { Clock } from "lucide-react";
import type { AppConfig } from "../../lib/types";
import { sleepThresholdMinutes, tabSleepSettings } from "../../lib/tabSleep";
import { SettingsCard, Toggle } from "./shared";

export function TabSleepCard({ config, onUpdate }: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
}) {
  const settings = tabSleepSettings(config);
  return <SettingsCard icon={<Clock size={16} />} title="Tab sleep"
    description="Idle agents sleep to save resources. Interacting with a tab resumes its session.">
    <Toggle label="Sleep idle tabs" checked={settings.enabled}
      onChange={enabled => onUpdate("tab_sleep", { ...settings, enabled })} />
    <label className="flex items-center justify-between gap-3 text-xs">
      Idle threshold (minutes)
      <input type="number" min={1} step={1} value={settings.idle_minutes} disabled={!settings.enabled}
        className="w-24 rounded border border-[var(--ct-border)] bg-[var(--ct-bg)] px-3 py-1.5"
        onInput={event => {
          const minutes = sleepThresholdMinutes(event.currentTarget.value);
          if (minutes !== null) onUpdate("tab_sleep", { ...settings, idle_minutes: minutes });
        }} />
    </label>
  </SettingsCard>;
}
