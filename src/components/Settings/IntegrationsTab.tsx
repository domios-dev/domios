import { useState } from "react";
import type { AppConfig } from "../../lib/types";
import { Toggle } from "./shared";

export function IntegrationsTab({ config, onUpdate }: { config: AppConfig; onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void }) {
  const [allowedUsersText, setAllowedUsersText] = useState(
    (config.integrations?.telegram_allowed_users ?? []).join(", ")
  );

  return (
    <div className="space-y-5">
      <div className="rounded-xl border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-3">
        <div className="flex items-center justify-between">
          <div className="text-xs font-medium text-[var(--ct-fg)]">Telegram</div>
          {config.integrations?.telegram_enabled && (
            <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-[var(--ct-green)]/20 text-[var(--ct-green)]">enabled</span>
          )}
        </div>
        <div className="text-xs text-[var(--ct-muted-fg)]">
          Send messages to your General Agent via Telegram bot. Responses are captured and sent back.
        </div>
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
      </div>

      <div className="rounded-xl border border-[var(--ct-border)] bg-[var(--ct-bg)]/50 p-4 space-y-3 opacity-50">
        <div className="flex items-center justify-between">
          <div className="text-xs font-medium text-[var(--ct-fg)]">Discord</div>
          <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-[var(--ct-border)] text-[var(--ct-muted-fg)]">coming soon</span>
        </div>
        <div className="text-xs text-[var(--ct-muted-fg)]">
          Connect a Discord bot to interact with your General Agent.
        </div>
      </div>
    </div>
  );
}
