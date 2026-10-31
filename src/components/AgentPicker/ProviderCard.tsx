import { AgentIcon } from "../AgentIcon";
import type { ProviderDescriptor } from "../../lib/types";
import { providerAvailability } from "../../lib/providerAvailability";

export const PROVIDER_CARD_LAYOUT = "flex min-h-[8.5rem] min-w-[9.5rem] flex-col items-center justify-center gap-2.5 px-4 py-4";

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
      className={`agent-provider-card ct-panel-card relative cursor-pointer ${PROVIDER_CARD_LAYOUT}`}
    >
      {provider.badge && <span className="agent-provider-card-badge">{provider.badge}</span>}
      <span className="agent-provider-card-selection" aria-hidden="true">
        ✓
      </span>
      <AgentIcon providerId={provider.id} className="text-[2rem]" />
      <span className="text-[13px] font-semibold text-[var(--ct-fg)]">{provider.display_name}</span>
    </button>
  );
}
