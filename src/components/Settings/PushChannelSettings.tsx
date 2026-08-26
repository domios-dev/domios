import type { AppConfig } from "../../lib/types";
import type { PushChannelDescriptor } from "../../lib/notifyDestinations";
import { audienceIsAChoice, configuredUsers } from "../../lib/pushChannels";
import { ChannelAudience } from "./ChannelAudience";

/**
 * The settings each offerable channel owns. Every channel answers the same one
 * question — who receives what the picker routed here — so there is no
 * per-channel editor to register: a channel the registry gains gets its audience
 * surface from the roster entry alone.
 *
 * And it gets one only where there is something to answer. With a single
 * admitted id, picking the channel for a level IS the audience, and confirming
 * a set of one is the same redundancy as typing an id that was already
 * configured. With none the channel is not offerable at all.
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
        .filter(
          (channel) =>
            channel.control_available && audienceIsAChoice(configuredUsers(config, channel.id)),
        )
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
