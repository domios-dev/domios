import { Sparkles } from "lucide-react";
import type { AppConfig, DecisionRerankConfig, MemoryConfig } from "../../lib/types";
import { InlineLinkButton, SettingsCard, Toggle } from "../Settings/shared";

interface Props {
  mem: MemoryConfig;
  onUpdate: <K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>) => void;
  onOpenAgentsSettings: () => void;
}

export function DecisionRerankCard({ mem, onUpdate, onOpenAgentsSettings }: Props) {
  return (
    <SettingsCard
      icon={<Sparkles size={16} />}
      title="Re-rank"
      description="The Supporter Agent re-orders memory recall so the results that answer come first."
    >
      <Toggle
        label="Re-rank memory recall"
        checked={mem.decision_rerank?.enabled ?? false}
        onChange={(v) =>
          onUpdate("memory", {
            ...mem,
            decision_rerank: { ...(mem.decision_rerank ?? {}), enabled: v } as DecisionRerankConfig,
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
