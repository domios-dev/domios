import { AgentIcon } from "../AgentIcon";
import type { ProviderDescriptor } from "../../lib/types";
import { providerAvailability } from "../../lib/providerAvailability";
import { providerCardState } from "../../lib/providerCardState";

export const PROVIDER_CARD_LAYOUT = "flex size-[9.5rem] flex-none flex-col items-center justify-center gap-2 px-3 py-4";

interface ProviderCardProps {
  provider: ProviderDescriptor;
  selected: boolean;
  onSelect: () => void;
  signInNeeded?: boolean;
}

export function ProviderCard({ provider, selected, onSelect, signInNeeded }: ProviderCardProps) {
  const availability = providerAvailability(provider);
  const status = providerCardState(provider, signInNeeded);
  return (
    <button
      type="button"
      data-testid={`agent-picker-card-${provider.id}`}
      data-availability={availability}
      data-selected={selected ? "yes" : "no"}
      aria-pressed={selected}
      aria-label={`${provider.display_name}: ${status.label}`}
      aria-busy={provider.availability_checking || undefined}
      onClick={onSelect}
      className={`agent-provider-card ct-panel-card relative cursor-pointer ${PROVIDER_CARD_LAYOUT}`}
    >
      {provider.badge && <span className="agent-provider-card-badge">{provider.badge}</span>}
      <span className="agent-provider-card-selection" aria-hidden="true">
        ✓
      </span>
      <AgentIcon providerId={provider.id} className="text-[2rem]" />
      <span className="flex w-full flex-col items-center leading-tight" title={`${provider.display_name}: ${status.label}`}>
        <span className="w-full truncate text-[13px] font-semibold text-[var(--ct-fg)]">{provider.display_name}</span>
        <span className={`w-full truncate text-[11px] text-[var(--ct-muted-fg)] ${status.state === "loading" ? "motion-safe:animate-pulse" : ""}`}>{status.label}</span>
      </span>
    </button>
  );
}
