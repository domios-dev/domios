import type { AppConfig } from "../../lib/types";
import type { PushChannelDescriptor } from "../../lib/notifyDestinations";
import { ChannelAudience } from "./ChannelAudience";

/**
 * The settings each offerable channel owns. Every channel answers the same one
 * question — who receives what the grid routed here — so there is no per-channel
 * editor to register: a channel the registry gains gets its audience surface
 * from the roster entry alone.
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
        .map((channel) => (
          <ChannelAudience
            key={channel.id}
            channel={channel}
            config={config}
            onUpdate={onUpdate}
          />
        ))}
    </>
  );
}
