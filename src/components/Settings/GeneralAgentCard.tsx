import type { HelpSectionId } from "../../help/types";
import { Bot } from "lucide-react";
import { invoke } from "../../lib/bridge";
import { GeneralAgentProviderChoice } from "./GeneralAgentProviderChoice";
import { Button } from "../shared/Button";
import { SettingsCard, Toggle } from "./shared";
import type { AppConfig } from "../../lib/types";
import { capabilityHintsEnabled, capabilityHintsPatch } from "./generalAgentSettingsModel";
import { firstRunGate } from "../../lib/firstRunGate";

interface GeneralAgentCardProps {
  config: AppConfig;
  onOpenGuide?: (section: HelpSectionId) => void;
  onSwitchProvider?: (provider: string) => Promise<void>;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
}

export function GeneralAgentCard({ config, onUpdate, onSwitchProvider, onOpenGuide }: GeneralAgentCardProps) {
  const gate = firstRunGate(config.general_agent);
  return (
    <SettingsCard
      icon={<Bot size={16} />}
      title="General Agent"
      description="Always-on AI assistant in a protected workspace. Starts automatically with Domios. Changing provider asks before restarting your General Agent."
    >
      <Toggle
        label="Enable General Agent"
        checked={config.general_agent?.enabled ?? true}
        onChange={(v) => onUpdate("general_agent", { enabled: v })}
      />
      <Toggle
        label="Suggest Domios features I have when relevant"
        checked={capabilityHintsEnabled(config.general_agent)}
        onChange={(enabled) => onUpdate("general_agent", capabilityHintsPatch(enabled))}
      />
      {config.general_agent?.enabled && (
        <div className="space-y-3">
            {onSwitchProvider && <GeneralAgentProviderChoice current={config.general_agent?.agent_provider}
              enabled={(config.general_agent?.enabled ?? true) && gate.allows("app")} onSwitch={onSwitchProvider} onOpenGuide={onOpenGuide} />}

            <Button
              variant="ghost"
              size="compact"
              data-testid="settings-general-agent-restart"
              disabled={!gate.allows("app")}
              onClick={() => {
                if (!gate.allows("app")) return;
                invoke("restart_general_agent").catch((e) => console.error("GA restart failed:", e));
              }}
              className="text-[11px]"
            >
              Restart
            </Button>
        </div>
      )}
    </SettingsCard>
  );
}
