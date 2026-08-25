import { useState } from "react";
import type { AppConfig } from "../../lib/types";
import { channelAudience, channelPatch, parseRecipients } from "../../lib/pushChannels";
import { Toggle } from "./shared";

/** The service id this editor belongs to, and the same id the daemon's registry
 *  and `codeterm bridge send --service` use. */
export const TELEGRAM_CHANNEL_ID = "telegram";

/**
 * Telegram's own push settings.
 *
 * Whether this renders at all is the registry's answer, not this component's:
 * it is mounted only for a channel the daemon reported as offerable. The
 * recipient list here is an audience and is deliberately separate from
 * `integrations.telegram_allowed_users`, which admits inbound senders.
 */
export function TelegramNotifications({
  config,
  onUpdate,
}: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
}) {
  const audience = channelAudience(config, TELEGRAM_CHANNEL_ID);
  const [recipientsText, setRecipientsText] = useState((audience?.recipients ?? []).join(", "));

  const patch = (next: Parameters<typeof channelPatch>[2]) =>
    onUpdate(
      "notifications",
      channelPatch(config.notifications?.channels, TELEGRAM_CHANNEL_ID, next),
    );

  return (
    <div className="flex flex-col gap-2" data-testid="settings-telegram-notifications">
      <Toggle
        label="Send agent alerts to Telegram"
        checked={audience?.enabled ?? false}
        onChange={(value) => patch({ enabled: value })}
      />
      {audience?.enabled && (
        <>
          <div className="space-y-1">
            <label
              className="block text-xs text-[var(--ct-muted-fg)]"
              htmlFor="telegram-push-recipients"
            >
              Recipient chat IDs (comma-separated)
            </label>
            <input
              id="telegram-push-recipients"
              type="text"
              data-testid="settings-telegram-push-recipients"
              value={recipientsText}
              onChange={(event) => {
                setRecipientsText(event.target.value);
                patch({ recipients: parseRecipients(event.target.value) });
              }}
              placeholder="e.g. 123456789"
              className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none font-mono"
            />
          </div>
          <p className="text-[10px] text-[var(--ct-muted)]">
            Only the levels you allow can be sent. Nobody receives anything
            until a chat ID is listed here.
          </p>
        </>
      )}
    </div>
  );
}
