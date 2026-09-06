import { type ReactNode, useEffect, useId, useMemo, useState } from "react";
import { invoke } from "../../lib/bridge";
import type { ProviderDescriptor } from "../../lib/types";
import { ProviderCard, ProviderInstallPanel, providerInstallSortKey } from "./ProviderCard";
import {
  hasAnySelectableProvider,
  isProviderSelectable,
  preselectProviderId,
} from "./preselectProvider";
import { useProviderAvailability } from "./useProviderAvailability";
import { Button } from "../shared/Button";

interface AgentProviderPickerProps {
  headline: string;
  providers: ProviderDescriptor[];
  confirmLabel?: string;
  busy?: boolean;
  children?: ReactNode;
  onConfirm: (providerId: string) => void;
}

export function AgentProviderPicker({
  headline,
  providers,
  confirmLabel = "Confirm",
  busy,
  children,
  onConfirm,
}: AgentProviderPickerProps) {
  const headlineId = useId();
  const { observed, state, refresh } = useProviderAvailability(providers);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [installBusy, setInstallBusy] = useState(false);

  const anySelectable = useMemo(() => hasAnySelectableProvider(observed), [observed]);

  useEffect(() => {
    if (state !== "ready") return;
    setSelectedId((cur) => cur ?? preselectProviderId(observed));
  }, [state, observed]);

  const sortedForInstall = useMemo(
    () => [...observed].sort((a, b) => providerInstallSortKey(a) - providerInstallSortKey(b)),
    [observed],
  );

  const onInstall = async (providerId: string) => {
    setInstallBusy(true);
    try {
      await invoke("install_provider_in_tab", { providerId });
    } finally {
      setInstallBusy(false);
    }
  };

  const canConfirm =
    Boolean(selectedId) &&
    anySelectable &&
    observed.some((p) => p.id === selectedId && isProviderSelectable(p));

  return (
    <div
      role="region"
      aria-labelledby={headlineId}
      data-testid="agent-provider-picker"
      className="flex h-full w-full items-center justify-center p-6 overflow-auto"
    >
      <div className="w-full max-w-2xl space-y-6">
        <div className="text-center space-y-1">
          <h1 id={headlineId} className="text-lg font-semibold text-[var(--ct-fg)]">{headline}</h1>
          <p className="text-xs text-[var(--ct-muted-fg)]">
            Pick the agent that powers your General tab. You can open plain terminal tabs from the
            sidebar anytime.
          </p>
        </div>

        {anySelectable ? (
          <div
            role="group"
            aria-label="Agent providers"
            className="flex flex-wrap justify-center gap-3"
            data-testid="agent-picker-cards"
          >
            {observed.map((provider) => (
              <ProviderCard
                key={provider.id}
                provider={provider}
                selected={selectedId === provider.id}
                disabled={!isProviderSelectable(provider)}
                onSelect={() => setSelectedId(provider.id)}
              />
            ))}
          </div>
        ) : (
          <div className="space-y-3" data-testid="agent-picker-install-fallback">
            <p className="text-xs text-center text-[var(--ct-muted-fg)]">
              No agent CLI could be found on this machine. Install one below, then re-check.
            </p>
            {sortedForInstall.map((provider) => (
              <ProviderInstallPanel
                key={provider.id}
                provider={provider}
                checking={state === "checking"}
                busy={installBusy}
                onRecheck={() => void refresh()}
                onInstall={() => void onInstall(provider.id)}
              />
            ))}
          </div>
        )}

        {anySelectable && children}

        {anySelectable && (
          <div className="flex justify-center">
            <Button
              variant="primary"
              size="lg"
              data-testid="agent-picker-confirm"
              disabled={!canConfirm || state === "checking"}
              loading={busy}
              onClick={() => selectedId && onConfirm(selectedId)}
            >
              {confirmLabel}
            </Button>
          </div>
        )}
      </div>
    </div>
  );
}
