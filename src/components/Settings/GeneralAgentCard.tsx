import { Bot } from "lucide-react";
import { invoke } from "../../lib/bridge";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { Button } from "../shared/Button";
import { SettingsCard, Toggle } from "./shared";
import type { AppConfig } from "../../lib/types";

interface GeneralAgentCardProps {
  config: AppConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
}

export function GeneralAgentCard({ config, onUpdate }: GeneralAgentCardProps) {
  const { providers } = useAgentProviders();

  return (
    <SettingsCard
      icon={<Bot size={16} />}
      title="General Agent"
      description="Always-on AI assistant in a protected workspace. Starts automatically with CodeTerm. Changes provider on next restart, or click Restart now."
    >
      <Toggle
        label="Enable General Agent"
        checked={config.general_agent?.enabled ?? true}
        onChange={(v) => onUpdate("general_agent", { enabled: v })}
      />
      {config.general_agent?.enabled && (
        <div className="flex items-center justify-between">
          <span className="text-xs text-[var(--ct-muted-fg)]">Provider</span>
          <div className="flex items-center gap-2">
            <select
              data-testid="settings-general-agent-provider"
              value={config.general_agent?.agent_provider ?? providers[0]?.id ?? ""}
              onChange={(e) => onUpdate("general_agent", { agent_provider: e.target.value })}
              className="bg-[var(--ct-bg)] border border-[var(--ct-border)] rounded px-3 py-1.5 text-xs text-[var(--ct-fg)] focus:border-[var(--ct-accent)] focus:outline-none"
            >
              {providers.map((p) => (
                <option key={p.id} value={p.id}>{p.display_name}</option>
              ))}
            </select>
            <Button
              variant="ghost"
              size="compact"
              data-testid="settings-general-agent-restart"
              onClick={() => {
                invoke("restart_general_agent").catch((e) => console.error("GA restart failed:", e));
              }}
              className="text-[11px]"
            >
              Restart
            </Button>
          </div>
        </div>
      )}
    </SettingsCard>
  );
}
