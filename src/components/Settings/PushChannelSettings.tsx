import type { ReactElement } from "react";
import type { AppConfig } from "../../lib/types";
import type { PushChannelDescriptor } from "../../lib/notifyDestinations";
import { TelegramNotifications, TELEGRAM_CHANNEL_ID } from "./TelegramNotifications";

export interface PushChannelEditorProps {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
}

/**
 * The JS side of the channel registry: each channel's own settings surface,
 * keyed by the same service id the daemon registers it under. A channel with
 * no entry is still a grid column — it simply keeps its audience elsewhere and
 * has nothing of its own to configure here.
 */
export const PUSH_CHANNEL_EDITORS: Record<
  string,
  (props: PushChannelEditorProps) => ReactElement | null
> = {
  [TELEGRAM_CHANNEL_ID]: TelegramNotifications,
};

/**
 * Renders the settings each offerable channel owns. Which channels exist and
 * which may be offered are both the daemon's answers, so adding a channel here
 * is one registry entry plus that channel's own component.
 */
export function PushChannelSettings({
  channels,
  config,
  onUpdate,
}: {
  channels: readonly PushChannelDescriptor[];
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
}) {
  return (
    <>
      {channels
        .filter((channel) => channel.control_available)
        .map((channel) => {
          const Editor = PUSH_CHANNEL_EDITORS[channel.id];
          if (!Editor) return null;
          return <Editor key={channel.id} config={config} onUpdate={onUpdate} />;
        })}
    </>
  );
}
