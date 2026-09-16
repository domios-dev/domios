import { AgentIcon } from "../AgentIcon";
import type { ProviderDescriptor } from "../../lib/types";
import { providerAvailability } from "../../lib/providerAvailability";

interface ProviderCardProps {
  provider: ProviderDescriptor;
  selected: boolean;
  onSelect: () => void;
}

export function ProviderCard({ provider, selected, onSelect }: ProviderCardProps) {
  const availability = providerAvailability(provider);
  return (
    <button
      type="button"
      data-testid={`agent-picker-card-${provider.id}`}
      data-availability={availability}
      data-selected={selected ? "yes" : "no"}
      aria-pressed={selected}
      onClick={onSelect}
      className="agent-provider-card ct-liquid-interactive ct-panel-card relative flex min-h-[6.8rem] min-w-[7.2rem] cursor-pointer flex-col items-center justify-center gap-2 px-4 py-3.5"
    >
      {provider.badge && <span className="agent-provider-card-badge">{provider.badge}</span>}
      <span className="agent-provider-card-selection" aria-hidden="true">
        ✓
      </span>
      <AgentIcon providerId={provider.id} className="text-2xl" />
      <span className="text-xs font-medium text-[var(--ct-fg)]">{provider.display_name}</span>
    </button>
  );
}
