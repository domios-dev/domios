import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import logoUrl from "../../assets/logo.png";
import { installSource, launchProviderInstaller } from "../../lib/providerInstall";
import { ProviderCard } from "./ProviderCard";
import { preselectProviderId, providerAvailability } from "./preselectProvider";
import { useProviderAvailability } from "./useProviderAvailability";
import { listen } from "../../lib/bridge";
import { PROVIDER_AVAILABILITY_CHANGED, type ProviderAvailabilityChange } from "../../lib/providerAvailability";
import { Button } from "../shared/Button";
import { shouldReturnToPicker, type PickerNavigation, type TabRef } from "./pickerNavigation";

interface AgentProviderPickerProps {
  headline: string;
  subhead: string;
  providers: ProviderDescriptor[];
  confirmLabel?: string;
  busy?: boolean;
  retry?: boolean;
  onConfirm: (providerId: string) => void;
  onInstall?: (providerId: string) => void;
  progress?: string | null;
  children?: ReactNode;
  belowProviders?: ReactNode;
  navigation?: PickerNavigation;
  /** The tab hosting this picker, where a successful install returns the user. */
  home?: TabRef;
}

export function AgentProviderPicker({
  headline,
  subhead,
  providers,
  confirmLabel = "Confirm",
  busy,
  retry,
  onConfirm,
  onInstall: installOverride,
  progress,
  children,
  belowProviders,
  navigation,
  home,
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
  const installerTab = useRef<string | null>(null);
  const route = useRef({ navigation, home });
  route.current = { navigation, home };
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    const refresh = () => { if (active) void silentRef.current(); };
    const returnHome = ({ payload }: { payload: ProviderAvailabilityChange }) => {
      const { navigation: nav, home: target } = route.current;
      if (!active || !nav || !target || !shouldReturnToPicker(payload, installerTab.current, nav.activeTabId())) return;
      installerTab.current = null;
      void nav.show(target);
    };
    window.addEventListener("focus", refresh);
    void listen<ProviderAvailabilityChange>(PROVIDER_AVAILABILITY_CHANGED, returnHome)
      .then((stop) => { if (active) unlisten = stop; else stop(); })
      .catch(() => {});
    return () => { active = false; unlisten?.(); window.removeEventListener("focus", refresh); };
  }, []);

  const onInstall = async (providerId: string) => {
    setInstallBusy(true);
    setInstallError(null);
    try {
      const launch = await launchProviderInstaller(providerId);
      installerTab.current = launch.tab_id;
      const workspaceId = launch.workspace_id ?? home?.workspaceId;
      if (navigation && workspaceId) await navigation.show({ workspaceId, tabId: launch.tab_id });
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
      {/* One centred column: the header, the cards and the confirm share an axis however many providers are shown. */}
      <div className="flex w-fit max-w-full flex-col items-center gap-6" data-testid="agent-picker-group">
        {installError && <InlineErrorNotice error={installError} operation="installing this provider" blocking />}
        <div className="flex max-w-full items-center gap-3">
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

        {progress && <p role="status" aria-live="polite">{progress}</p>}
        {children}

        <div
          role="group"
          aria-label="Agent providers"
          className="flex max-w-full flex-wrap justify-center gap-3"
          data-testid="agent-picker-cards"
        >
          {observed.map((provider) => (
            <ProviderCard
              key={provider.id}
              provider={provider}
              selected={selectedId === provider.id}
              onSelect={() => { if (!busy && !installBusy) setSelectedId(provider.id); }}
            />
          ))}
        </div>
        {belowProviders}

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
                if (needsInstall) { if (installOverride) installOverride(selected.id); else void onInstall(selected.id); }
                else onConfirm(selected.id);
              }}
            >
              {retry ? "Retry" : needsInstall && selected ? `Install ${selected.display_name}` : confirmLabel}
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
