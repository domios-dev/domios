import { Bell } from "lucide-react";
import type { AppConfig } from "../../lib/types";
import { ProviderNotifications } from "./ProviderNotifications";
import { NotificationDestinations } from "./NotificationDestinations";
import { NotificationSurfaces } from "./NotificationSurfaces";
import { TelegramNotifications } from "./TelegramNotifications";
import { SettingsCard, Toggle } from "./shared";

export function NotificationsOverview({
  config,
  onUpdate,
  onUpdateFullConfig,
}: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
  onUpdateFullConfig: (next: AppConfig) => void;
}) {
  return (
    <div className="flex flex-col gap-5" data-testid="settings-notifications-overview">
      <SettingsCard
        icon={<Bell size={16} />}
        title="Notifications"
        description="Get alerted when an agent finishes a task or needs your attention. Hooks auto-install on startup."
      >
        <Toggle
          label="Show the bell"
          checked={config.notifications.show_bell !== false}
          onChange={(v) => onUpdate("notifications", { show_bell: v })}
        />
        <p className="text-[10px] text-[var(--ct-muted)] leading-relaxed">
          Off hides the indicator entirely. History is still kept, and the panel stays the complete
          log.
        </p>
        <NotificationSurfaces config={config} onUpdate={onUpdate} />
        <label className="flex items-center justify-between opacity-50 cursor-not-allowed">
          <span className="text-xs text-[var(--ct-muted-fg)]">In-app toast (coming soon)</span>
          <span
            className="relative inline-flex h-5 w-9 items-center rounded-full bg-[var(--ct-border)]"
          >
            <span className="inline-block h-3.5 w-3.5 rounded-full bg-white translate-x-0.5" />
          </span>
        </label>

        <h4 className="text-[var(--ct-muted-fg)] text-xs font-medium pt-1">
          Where each level may go
        </h4>
        <NotificationDestinations config={config} onUpdate={onUpdate} />

        <TelegramNotifications config={config} onUpdate={onUpdate} />

        <ProviderNotifications
          fullConfig={config}
          onUpdateFullConfig={onUpdateFullConfig}
          hideActions
          hidePathLinks
        />
      </SettingsCard>
    </div>
  );
}
