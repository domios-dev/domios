import { useEffect, useState } from "react";
import { invoke } from "../../lib/bridge";
import type { AppConfig } from "../../lib/types";
import { Toggle } from "./shared";

/**
 * Opt-in Telegram push for agent notifications.
 *
 * The control renders only when the daemon says a push could physically be
 * carried: a live bridge supervisor AND a non-empty inbound allow list. The
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
  const telegram = config.notifications?.telegram;
  const [controlAvailable, setControlAvailable] = useState(false);
  const [recipientsText, setRecipientsText] = useState((telegram?.recipients ?? []).join(", "));

  useEffect(() => {
    let cancelled = false;
    invoke<{ control_available?: boolean }>("telegram_push_gate")
      .then((gate) => {
        if (!cancelled) setControlAvailable(Boolean(gate?.control_available));
      })
      .catch(() => {
        if (!cancelled) setControlAvailable(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (!controlAvailable) return null;

  const patch = (next: Partial<NonNullable<typeof telegram>>) =>
    onUpdate("notifications", {
      telegram: {
        enabled: telegram?.enabled ?? false,
        recipients: telegram?.recipients ?? [],
        per_scope_limit: telegram?.per_scope_limit ?? 12,
        ...next,
      },
    });

  return (
    <div className="flex flex-col gap-2" data-testid="settings-telegram-notifications">
      <Toggle
        label="Send agent alerts to Telegram"
        checked={telegram?.enabled ?? false}
        onChange={(value) => patch({ enabled: value })}
      />
      {telegram?.enabled && (
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
                patch({
                  recipients: event.target.value
                    .split(",")
                    .map((entry) => entry.trim())
                    .filter(Boolean),
                });
              }}
              placeholder="e.g. 123456789"
              className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none font-mono"
            />
          </div>
          <p className="text-[10px] text-[var(--ct-muted)]">
            Only milestones, blockers and failures can be sent. Routine progress cannot. Nobody
            receives anything until a chat ID is listed here.
          </p>
        </>
      )}
    </div>
  );
}
