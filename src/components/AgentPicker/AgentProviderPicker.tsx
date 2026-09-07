import { type ReactNode, useEffect, useId, useMemo, useState } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import { launchProviderInstaller } from "../../lib/providerInstall";
import { ProviderCard, ProviderInstallPanel, providerInstallSortKey } from "./ProviderCard";
import {
  hasAnyInstalledProvider,
  isProviderInstalled,
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
  const { installedByBinary, state, refresh } = useProviderAvailability(providers);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [installBusy, setInstallBusy] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);

  const anyInstalled = useMemo(
    () => hasAnyInstalledProvider(providers, installedByBinary),
    [providers, installedByBinary],
  );

  useEffect(() => {
    if (state !== "ready") return;
    setSelectedId((cur) => cur ?? preselectProviderId(providers, installedByBinary));
  }, [state, providers, installedByBinary]);

  const sortedForInstall = useMemo(
    () => [...providers].sort((a, b) => providerInstallSortKey(a) - providerInstallSortKey(b)),
    [providers],
  );

  const onInstall = async (providerId: string) => {
    setInstallBusy(true);
    setInstallError(null);
    try {
      await launchProviderInstaller(providerId);
    } catch (error) {
      setInstallError(error instanceof Error ? error.message : String(error));
    } finally {
      setInstallBusy(false);
    }
  };

  const canConfirm =
    Boolean(selectedId) &&
    anyInstalled &&
    providers.some(
      (p) => p.id === selectedId && isProviderInstalled(p, installedByBinary),
    );

  return (
    <div
      role="region"
      aria-labelledby={headlineId}
      data-testid="agent-provider-picker"
      className="flex h-full w-full items-center justify-center p-6 overflow-auto"
    >
      <div className="w-full max-w-2xl space-y-6">
        {installError && <div role="alert" className="text-sm text-[var(--ct-red)]">{installError}</div>}
        <div className="text-center space-y-1">
          <h1 id={headlineId} className="text-lg font-semibold text-[var(--ct-fg)]">{headline}</h1>
          <p className="text-xs text-[var(--ct-muted-fg)]">
            Pick the agent that powers your General tab. You can open plain terminal tabs from the
            sidebar anytime.
          </p>
        </div>

        {anyInstalled ? (
          <div
            role="group"
            aria-label="Agent providers"
            className="flex flex-wrap justify-center gap-3"
            data-testid="agent-picker-cards"
          >
            {providers.map((provider) => (
              <ProviderCard
                key={provider.id}
                provider={provider}
                selected={selectedId === provider.id}
                installed={isProviderInstalled(provider, installedByBinary)}
                disabled={!isProviderInstalled(provider, installedByBinary)}
                onSelect={() => setSelectedId(provider.id)}
              />
            ))}
          </div>
        ) : (
          <div className="space-y-3" data-testid="agent-picker-install-fallback">
            <p className="text-xs text-center text-[var(--ct-muted-fg)]">
              No agent CLIs detected. Install one below, then re-check.
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

        {anyInstalled && children}

        {anyInstalled && (
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
