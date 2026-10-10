import { Sparkles } from "lucide-react";
import { InlineLinkButton, SettingsCard } from "../Settings/shared";

interface Props {
  onOpenAgentsSettings: () => void;
}

export function DecisionRerankCard({ onOpenAgentsSettings }: Props) {
  return (
    <SettingsCard
      icon={<Sparkles size={16} />}
      title="Re-rank"
      description="The Spinal Cord re-orders memory recall so the results that answer come first."
    >
      <p className="text-xs text-[var(--ct-muted-fg)]">Memory recall uses the Spinal Cord whenever its adapter is connected.</p>
      <InlineLinkButton
        label="Set up the Spinal Cord in Settings > Agents"
        onClick={onOpenAgentsSettings}
        testId="memory-rerank-agents-link"
      />
    </SettingsCard>
  );
}
