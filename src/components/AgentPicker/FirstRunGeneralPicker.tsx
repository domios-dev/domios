import { InlineErrorNotice } from "../shared/ErrorNotice";
import { ChartColumn } from "lucide-react";
import { useMemo } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import { AgentProviderPicker } from "./AgentProviderPicker";
import { generalAgentProviders } from "./preselectProvider";
import { usePickerNavigation, type TabRef } from "./pickerNavigation";
import { RuntimeLoginActions } from "../Terminal/RuntimeLoginActions";
import { useFirstRunLogin } from "./useFirstRunLogin";
import { useFirstRunStart } from "./useFirstRunStart";
import { FirstRunSetupProgress } from "./FirstRunSetupProgress";
import { useAnalyticsConsent } from "../Settings/useAnalyticsConsent";

export function FirstRunGeneralPicker({ providers, providersLoading = false, providersError, home }: {
  providers: ProviderDescriptor[];
  providersLoading?: boolean;
  providersError?: string | null;
  home?: TabRef;
}) {
  const eligibleProviders = useMemo(() => generalAgentProviders(providers), [providers]);
  const navigation = usePickerNavigation();
  const { enabled: analyticsOn } = useAnalyticsConsent();

  const { login, beforeStart } = useFirstRunLogin(home, eligibleProviders);
  const { busy, phase, error, progress, onConfirm, onInstall } = useFirstRunStart(navigation.show, beforeStart);

  return (
    <div className="h-full w-full bg-[var(--ct-bg)]" data-testid="first-run-general-picker">
      <AgentProviderPicker
        headline="Welcome to Domios"
        subhead="Pick the agent that runs your General tab. You can change it later in Settings."
        providers={eligibleProviders}
        providersLoading={providersLoading}
        confirmLabel="Start General agent"
        busy={busy}
        retry={Boolean(error)}
        onConfirm={onConfirm}
        onInstall={onInstall}
        progress={login ? "Sign in to continue" : progress}
        navigation={navigation}
        home={home}
        belowConfirm={analyticsOn && <p className="inline-flex items-center gap-1.5 text-[11px] text-[color-mix(in_srgb,var(--ct-muted-fg)_70%,transparent)]"><ChartColumn size={12} aria-hidden="true" />Product analytics is on. Turn off in <a className="underline" href="#settings/general" onClick={event => {
          event.preventDefault();
          window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "settings:general" } }));
        }}>Settings</a>.</p>}
      >
        {providersError && !providersLoading && <InlineErrorNotice error={providersError} operation="loading agent providers" blocking />}
        <FirstRunSetupProgress phase={phase} />
        {login && home && <RuntimeLoginActions tabId={home.tabId} providerId={login.providerId} state={login.state} />}
        {error && <div data-testid="first-run-error"><InlineErrorNotice error={error} operation="setting up the General agent" blocking /></div>}
      </AgentProviderPicker>
    </div>
  );
}
