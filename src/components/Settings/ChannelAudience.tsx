import type { AppConfig } from "../../lib/types";
import type { PushChannelDescriptor } from "../../lib/notifyDestinations";
import {
  channelPatch,
  channelRecipients,
  configuredUsers,
  recipientLabel,
  toggleRecipient,
} from "../../lib/pushChannels";
import { SettingsSection, SwitchTile } from "./shared";

/**
 * One channel's own settings surface, which answers WHO and never a second
 * WHETHER: whether a level reaches this channel is that level's push picker, and
 * there is no opt-in here to disagree with it.
 *
 * Drawn only where the question is real — several admitted ids — which its host
 * decides. The ids offered come from the service's inbound admission list, which
 * is a source of options and is deliberately not the audience: the admission
 * list says who may talk to us, the audience says who we push to. Selecting
 * nobody therefore reaches nobody; it never widens back to everyone admitted.
 */
export function ChannelAudience({
  channel,
  config,
  onUpdate,
}: {
  channel: PushChannelDescriptor;
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
}) {
  const offered = configuredUsers(config, channel.id);
  const selected = channelRecipients(config, channel.id);

  return (
    <SettingsSection title={`Who ${channel.label} writes to`}>
      <div
        className="flex flex-col gap-2"
        data-testid="settings-channel-audience"
        data-channel={channel.id}
        role="group"
        aria-label={`${channel.label} recipients`}
      >
        {offered.map((id) => (
          <SwitchTile
            key={id}
            dense
            label={recipientLabel(channel.label, id)}
            checked={selected.includes(id)}
            onChange={(value) =>
              onUpdate(
                "notifications",
                channelPatch(config.notifications?.channels, channel.id, {
                  recipients: toggleRecipient(selected, id, value),
                }),
              )
            }
          />
        ))}
      </div>
      <p className="text-[10px] text-[var(--ct-muted)] leading-relaxed">
        Selecting nobody means nobody. This list never falls back to everyone the
        bridge admits, and which levels arrive here is the picker above.
      </p>
    </SettingsSection>
  );
}
