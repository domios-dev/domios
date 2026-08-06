import { useState } from "react";
import { Send, MessageSquare } from "lucide-react";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import type { AppConfig } from "../../lib/types";
import {
  ProviderIntegrationHealthCard,
  providersWithIntegrationHealth,
} from "./ProviderIntegrationHealthCard";
import { SettingsCard, SettingsPill, Toggle } from "./shared";

export function IntegrationsTab({ config, onUpdate }: { config: AppConfig; onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void }) {
  const [allowedUsersText, setAllowedUsersText] = useState(
    (config.integrations?.telegram_allowed_users ?? []).join(", ")
  );
  const { providers } = useAgentProviders();
  const integrationProviders = providersWithIntegrationHealth(providers);

  return (
    <div className="flex flex-col gap-5" data-testid="settings-integrations-tab">
      {integrationProviders.map((provider) => (
        <ProviderIntegrationHealthCard key={provider.id} provider={provider} />
      ))}

      <SettingsCard
        icon={<Send size={16} />}
        title="Telegram"
        headerRight={
          config.integrations?.telegram_enabled ? (
            <SettingsPill tone="green">enabled</SettingsPill>
          ) : undefined
        }
        description="Send messages to your General Agent via Telegram bot. Responses are captured and sent back."
      >
        <Toggle
          label="Enable Telegram bridge"
          checked={config.integrations?.telegram_enabled ?? false}
          onChange={(v) => onUpdate("integrations", { telegram_enabled: v })}
        />
        {config.integrations?.telegram_enabled && (
          <>
            <div className="text-xs text-[var(--ct-muted-fg)]">
              Token: <span className="font-mono text-[var(--ct-fg)]">~/.codeterm/secrets/telegram.token</span>
            </div>
            <div className="space-y-1">
              <div className="text-xs text-[var(--ct-muted-fg)]">Allowed user IDs (comma-separated, empty = all)</div>
              <input
                type="text"
                data-testid="settings-telegram-allowed-users"
                value={allowedUsersText}
                onChange={(e) => {
                  setAllowedUsersText(e.target.value);
                  const ids = e.target.value.split(",").map((s) => s.trim()).filter(Boolean);
                  onUpdate("integrations", { telegram_allowed_users: ids });
                }}
                placeholder="e.g. 123456789, 987654321"
                className="w-full bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none font-mono"
              />
            </div>
            <div className="text-[10px] text-[var(--ct-muted)]">
              Requires restart. Create a bot via @BotFather and save the token.
            </div>
          </>
        )}
      </SettingsCard>

      <SettingsCard
        icon={<MessageSquare size={16} />}
        title="Discord"
        headerRight={<SettingsPill tone="muted">coming soon</SettingsPill>}
        description="Connect a Discord bot to interact with your General Agent."
        className="opacity-50"
      />
    </div>
  );
}
