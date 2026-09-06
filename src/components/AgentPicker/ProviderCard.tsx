import { AgentIcon } from "../AgentIcon";
import { Button } from "../shared/Button";
import { Card } from "../shared/Card";
import { Chip } from "../shared/Chip";
import type { ProviderDescriptor } from "../../lib/types";
import {
  providerAvailability,
  providerAvailabilityLabel,
} from "../../lib/providerAvailability";
import { PREFERRED_GENERAL_PROVIDER_ID } from "./constants";

interface ProviderCardProps {
  provider: ProviderDescriptor;
  selected: boolean;
  disabled?: boolean;
  onSelect: () => void;
}

export function ProviderCard({ provider, selected, disabled, onSelect }: ProviderCardProps) {
  const availability = providerAvailability(provider);
  return (
    <button
      type="button"
      data-testid={`agent-picker-card-${provider.id}`}
      data-availability={availability}
      data-selected={selected ? "yes" : "no"}
      aria-pressed={selected}
      disabled={disabled}
      onClick={onSelect}
      className={[
        "agent-provider-card ct-liquid-interactive ct-panel-card relative flex flex-col items-center gap-2 px-4 py-3 min-w-[7.5rem]",
        selected
          ? "text-[var(--ct-fg)]"
          : "text-[var(--ct-muted-fg)] hover:border-[var(--ct-accent)]/40 hover:text-[var(--ct-fg)]",
        disabled ? "opacity-50 cursor-not-allowed" : "cursor-pointer",
      ].join(" ")}
    >
      <span className="agent-provider-card-selection" aria-hidden="true">
        ✓
      </span>
      <AgentIcon providerId={provider.id} className="text-2xl" />
      <span className="text-xs font-medium">{provider.display_name}</span>
      <Chip variant={availability === "available" ? "success" : "muted"}>
        {providerAvailabilityLabel(provider)}
      </Chip>
    </button>
  );
}

interface ProviderInstallPanelProps {
  provider: ProviderDescriptor;
  onRecheck: () => void;
  onInstall: () => void;
  busy?: boolean;
  checking?: boolean;
}

export function ProviderInstallPanel({
  provider,
  onRecheck,
  onInstall,
  busy,
  checking,
}: ProviderInstallPanelProps) {
  const installCommand = provider.install_command;
  return (
    <Card
      role="group"
      aria-label={`Install ${provider.display_name}`}
      data-testid={`agent-picker-install-${provider.id}`}
      className="p-4 space-y-2"
    >
      <div className="flex items-center gap-2">
        <AgentIcon providerId={provider.id} className="text-lg" />
        <div className="min-w-0">
          <div className="text-xs font-medium text-[var(--ct-fg)]">{provider.display_name}</div>
          {provider.cli_binary && (
            <div className="text-[10px] text-[var(--ct-muted-fg)] font-mono truncate">
              {provider.cli_binary}
            </div>
          )}
        </div>
      </div>
      {installCommand ? (
        <div className="text-[10px] text-[var(--ct-muted)] font-mono bg-[var(--ct-bg)] rounded px-2 py-1.5 overflow-x-auto">
          {installCommand}
        </div>
      ) : (
        <div className="text-[10px] text-[var(--ct-muted-fg)]">
          No install command for this platform — install manually, then re-check.
        </div>
      )}
      <div className="flex items-center justify-end gap-2">
        <Button variant="ghost" size="sm" onClick={onRecheck} disabled={checking}>
          {checking ? "Checking…" : "Re-check"}
        </Button>
        {installCommand && (
          <Button variant="accent" size="sm" onClick={onInstall} disabled={busy} loading={busy}>
            Install
          </Button>
        )}
      </div>
    </Card>
  );
}

export function providerInstallSortKey(provider: ProviderDescriptor): number {
  return provider.id === PREFERRED_GENERAL_PROVIDER_ID ? 0 : 1;
}
