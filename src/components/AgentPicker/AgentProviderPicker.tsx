import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import logoUrl from "../../assets/logo.png";
import { installSource, launchProviderInstaller } from "../../lib/providerInstall";
import { ProviderCard } from "./ProviderCard";
import { preselectProviderId, providerAvailability } from "./preselectProvider";
import { useProviderAvailability } from "./useProviderAvailability";
import { listen } from "../../lib/bridge";
import { Button } from "../shared/Button";

interface AgentProviderPickerProps {
  headline: string;
  subhead: string;
  providers: ProviderDescriptor[];
  confirmLabel?: string;
  busy?: boolean;
  onConfirm: (providerId: string) => void;
}

export function AgentProviderPicker({
  headline,
  subhead,
  providers,
  confirmLabel = "Confirm",
  busy,
  onConfirm,
}: AgentProviderPickerProps) {
  const headlineId = useId();
  const { observed, state, refresh, silentRefresh } = useProviderAvailability(providers);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [installBusy, setInstallBusy] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);

  useEffect(() => {
    if (state !== "ready") return;
    setSelectedId((cur) => cur ?? preselectProviderId(observed) ?? observed[0]?.id ?? null);
  }, [state, observed]);

  const selected = useMemo(
    () => observed.find((p) => p.id === selectedId) ?? null,
    [observed, selectedId],
  );
  const needsInstall = selected ? providerAvailability(selected) === "unavailable" : false;

  const silentRef = useRef(silentRefresh);
  silentRef.current = silentRefresh;
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    const refresh = () => { if (active) void silentRef.current(); };
    window.addEventListener("focus", refresh);
    void listen("provider-cli-completed", refresh).then((stop) => { if (active) unlisten = stop; else stop(); }).catch(() => {});
    return () => { active = false; unlisten?.(); window.removeEventListener("focus", refresh); };
  }, []);

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

  return (
    <div
      role="region"
      aria-labelledby={headlineId}
      data-testid="agent-provider-picker"
      className="flex h-full w-full items-center justify-center p-6 overflow-auto"
    >
      <div className="w-fit max-w-full space-y-6">
        {installError && <InlineErrorNotice error={installError} operation="installing this provider" blocking />}
        <div className="flex items-center gap-3">
          <img src={logoUrl} alt="" className="h-8 w-8 flex-none rounded-lg" />
          <div className="min-w-0">
            <h1
              id={headlineId}
              className="text-[17px] font-semibold tracking-[-0.01em] text-[var(--ct-fg)]"
            >
              {headline}
            </h1>
            <p className="mt-0.5 text-[13px] leading-snug text-[var(--ct-muted-fg)]">{subhead}</p>
          </div>
        </div>

        <div
          role="group"
          aria-label="Agent providers"
          className="flex flex-wrap gap-3"
          data-testid="agent-picker-cards"
        >
          {observed.map((provider) => (
            <ProviderCard
              key={provider.id}
              provider={provider}
              selected={selectedId === provider.id}
              onSelect={() => setSelectedId(provider.id)}
            />
          ))}
        </div>

        {/* The command line is out of flow so the row never changes height;
            Re-check unfurls in flow so the pair spreads apart in one motion. */}
        <div className="flex items-center justify-center">
          <div className="relative">
            <Button
              variant="primary"
              size="lg"
              data-testid="agent-picker-confirm"
              disabled={!selected || state === "checking" || (needsInstall && !selected.install_command)}
              loading={busy || installBusy}
              onClick={() => {
                if (!selected) return;
                if (needsInstall) void onInstall(selected.id);
                else onConfirm(selected.id);
              }}
            >
              {needsInstall && selected ? `Install ${selected.display_name}` : confirmLabel}
            </Button>
            {needsInstall && installSource(selected?.install_command) && (
              <p className="absolute left-1/2 top-full mt-3 -translate-x-1/2 whitespace-nowrap text-center font-mono text-[11px] text-[var(--ct-muted-fg)]">
                {installSource(selected?.install_command)}
              </p>
            )}
          </div>
          <div className="ct-picker-aside" data-open={needsInstall ? "true" : "false"}>
            <div>
              <Button
                variant="ghost"
                size="lg"
                className="ml-3 whitespace-nowrap"
                tabIndex={needsInstall ? undefined : -1}
                aria-hidden={!needsInstall}
                onClick={() => void refresh()}
                disabled={!needsInstall || state === "checking"}
              >
                {state === "checking" ? "Checking…" : "Re-check"}
              </Button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
