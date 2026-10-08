import { useId, useState } from "react";
import { MessageCircle } from "lucide-react";
import type { AppConfig } from "../../lib/types";
import { FormRow } from "../shared/FormRow";
import { UI_INPUT } from "../shared/design";
import { SettingsCard, SettingsPill, Toggle } from "./shared";

export function parseAllowedUsers(text: string): string[] {
  return text.split(",").map((value) => value.trim()).filter(Boolean);
}

export function MessengersCard({
  config,
  onUpdate,
}: {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
}) {
  const usersId = useId();
  const enabled = config.integrations?.telegram_enabled ?? false;
  const [allowedUsersText, setAllowedUsersText] = useState(
    (config.integrations?.telegram_allowed_users ?? []).join(", "),
  );

  return (
    <SettingsCard
      icon={<MessageCircle size={16} />}
      title="Messengers"
      headerRight={enabled ? <SettingsPill tone="green">Telegram on</SettingsPill> : undefined}
      description="Talk to your General Agent from a chat app. Replies go back to the chat you wrote from."
    >
      <Toggle
        label="Telegram bot bridge"
        checked={enabled}
        onChange={(value) => onUpdate("integrations", { telegram_enabled: value })}
      />
      {enabled && (
        <>
          <FormRow label="Allowed user IDs" htmlFor={usersId} hint="Comma-separated Telegram user IDs. Empty allows anyone who finds the bot.">
            <input
              id={usersId}
              type="text"
              data-testid="settings-telegram-allowed-users"
              value={allowedUsersText}
              onChange={(event) => {
                setAllowedUsersText(event.target.value);
                onUpdate("integrations", { telegram_allowed_users: parseAllowedUsers(event.target.value) });
              }}
              placeholder="123456789, 987654321"
              className={UI_INPUT.fullMono}
            />
          </FormRow>
          <p className="m-0 text-[10px] text-[var(--ct-muted)]">
            Create a bot with @BotFather and save its token to{" "}
            <span className="ct-text-mono text-[var(--ct-muted-fg)]">~/.codeterm/secrets/telegram.token</span>. Restart Domios to apply.
          </p>
        </>
      )}
    </SettingsCard>
  );
}
