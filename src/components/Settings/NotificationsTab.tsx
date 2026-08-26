import { useEffect, useState } from "react";
import { Bell } from "lucide-react";
import { invoke } from "../../lib/bridge";
import type { AppConfig } from "../../lib/types";
import type { PushChannelDescriptor } from "../../lib/notifyDestinations";
import { ProviderNotifications } from "./ProviderNotifications";
import { NotificationDestinations } from "./NotificationDestinations";
import { NotificationSurfaces } from "./NotificationSurfaces";
import { PushChannelSettings } from "./PushChannelSettings";
import { SettingsCard } from "./shared";

/** The daemon's channel registry. A roster it cannot answer for is empty
 *  rather than guessed at: the grid then draws its two local surfaces, which
 *  is honest about what this build could resolve. */
export function usePushChannels(): PushChannelDescriptor[] {
  const [channels, setChannels] = useState<PushChannelDescriptor[]>([]);

  useEffect(() => {
    let cancelled = false;
    invoke<{ channels?: PushChannelDescriptor[] }>("push_channel_roster")
      .then((roster) => {
        if (!cancelled) setChannels(roster?.channels ?? []);
      })
      .catch(() => {
        if (!cancelled) setChannels([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return channels;
}

export function NotificationsOverview({
  config,
  onUpdate,
  onUpdateFullConfig,
}: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
  onUpdateFullConfig: (next: AppConfig) => void;
}) {
  const channels = usePushChannels();

  return (
    <div className="flex flex-col gap-5" data-testid="settings-notifications-overview">
      <SettingsCard
        icon={<Bell size={16} />}
        title="Notifications"
        description="Get alerted when an agent finishes a task or needs your attention. Hooks auto-install on startup."
      >
        <div className="flex flex-col gap-5">
          <NotificationSurfaces config={config} onUpdate={onUpdate} />
          <NotificationDestinations config={config} channels={channels} onUpdate={onUpdate} />
          <PushChannelSettings channels={channels} config={config} onUpdate={onUpdate} />
        </div>
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
