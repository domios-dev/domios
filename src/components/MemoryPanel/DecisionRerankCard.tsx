import { Sparkles } from "lucide-react";
import type { AppConfig, SupporterConfig } from "../../lib/types";
import { InlineLinkButton, SettingsCard, Toggle } from "../Settings/shared";

interface Props {
  supporter: SupporterConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
  onOpenAgentsSettings: () => void;
}

export function DecisionRerankCard({ supporter, onUpdate, onOpenAgentsSettings }: Props) {
  return (
    <SettingsCard
      icon={<Sparkles size={16} />}
      title="Re-rank"
      description="The Supporter Agent re-orders memory recall so the results that answer come first."
    >
      <Toggle
        label="Re-rank memory recall"
        checked={supporter.memory_rerank.enabled}
        onChange={(v) =>
          onUpdate("supporter", {
            ...supporter,
            memory_rerank: { ...supporter.memory_rerank, enabled: v },
          })
        }
      />
      <InlineLinkButton
        label="Set up the Supporter Agent in Settings > Agents"
        onClick={onOpenAgentsSettings}
        testId="memory-rerank-agents-link"
      />
    </SettingsCard>
  );
}
