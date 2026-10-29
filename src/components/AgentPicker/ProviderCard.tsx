import { useEffect, useRef } from "react";
import { AgentIcon } from "../AgentIcon";
import type { ProviderDescriptor } from "../../lib/types";
import { providerAvailability } from "../../lib/providerAvailability";
import { DotMatrixCanvas, type DotMatrixCanvasHandle } from "../brand/DotMatrixCanvas";
import { usePrefersReducedMotion } from "../brand/usePrefersReducedMotion";

export const PROVIDER_CARD_LAYOUT = "flex min-h-[8.5rem] min-w-[9.5rem] flex-col items-center justify-center gap-2.5 px-4 py-4";

interface ProviderCardProps {
  provider: ProviderDescriptor;
  selected: boolean;
  onSelect: () => void;
}

export function ProviderCard({ provider, selected, onSelect }: ProviderCardProps) {
  const availability = providerAvailability(provider);
  const dots = useRef<DotMatrixCanvasHandle>(null);
  const reducedMotion = usePrefersReducedMotion();
  // The chosen card answers with one wave from its glyph and keeps a lit field; hover stays still.
  useEffect(() => { if (selected) dots.current?.emitRelease(performance.now()); }, [selected]);
  return (
    <button
      type="button"
      data-testid={`agent-picker-card-${provider.id}`}
      data-availability={availability}
      data-selected={selected ? "yes" : "no"}
      aria-pressed={selected}
      onClick={onSelect}
      className={`agent-provider-card brand-dot-stage ct-liquid-interactive ct-panel-card relative cursor-pointer ${PROVIDER_CARD_LAYOUT}`}
    >
      {selected && <DotMatrixCanvas ref={dots} mode="loading" reducedMotion={reducedMotion} intensity={3} restIntensity={10} waveDurationMs={1_100} />}
      {provider.badge && <span className="agent-provider-card-badge">{provider.badge}</span>}
      <span className="agent-provider-card-selection" aria-hidden="true">
        ✓
      </span>
      <span data-dot-origin className="inline-flex"><AgentIcon providerId={provider.id} className="text-[2rem]" /></span>
      <span className="text-[13px] font-semibold text-[var(--ct-fg)]">{provider.display_name}</span>
    </button>
  );
}
