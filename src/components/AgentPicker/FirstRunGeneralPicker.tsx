import { InlineErrorNotice } from "../shared/ErrorNotice";
import { ProviderPrerequisitesNotice } from "../shared/ProviderPrerequisitesNotice";
import { CodeBlock } from "@codeterm/chat-ui/CodeBlock";
import { ChartColumn } from "lucide-react";
import { useMemo } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import { installerShellLanguage } from "../../lib/providerCli";
import { AgentProviderPicker } from "./AgentProviderPicker";
import { generalAgentProviders } from "./preselectProvider";
import { usePickerNavigation, type TabRef } from "./pickerNavigation";
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

  const { busy, phase, error, failedInstallProviderId, failedInstallPrerequisites, progress, onConfirm, onInstall } = useFirstRunStart(navigation.show);
  const failedProvider = failedInstallProviderId
    ? eligibleProviders.find((provider) => provider.id === failedInstallProviderId) ?? null
    : null;

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
        progress={progress}
        navigation={navigation}
        home={home}
        belowConfirm={analyticsOn && <p className="inline-flex items-center gap-1.5 text-[11px] text-[color-mix(in_srgb,var(--ct-muted-fg)_70%,transparent)]"><ChartColumn size={12} aria-hidden="true" />Product analytics is on. Turn off in <a className="underline" href="#settings/general" onClick={event => {
          event.preventDefault();
          window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: "settings:general" } }));
        }}>Settings</a>.</p>}
      >
        {providersError && !providersLoading && <InlineErrorNotice error={providersError} operation="loading agent providers" blocking />}
        <FirstRunSetupProgress phase={phase} />
        {error && <div data-testid="first-run-error" className="flex w-full max-w-lg flex-col gap-2">
          <InlineErrorNotice
            error={error}
            description={failedInstallProviderId ? {
              title: `${failedProvider?.display_name ?? "Provider"} could not be installed`,
              cause: error,
              actions: ["retry"],
              raw: error,
            } : undefined}
            operation="setting up the General agent"
            blocking
          />
          <ProviderPrerequisitesNotice prerequisites={failedInstallPrerequisites} />
          {failedProvider?.install_command && (
            <div aria-label="Manual install command" role="region" className="w-full min-w-0 text-left">
              <p className="mb-1 text-xs text-[var(--ct-muted-fg)]">Install manually, then click Retry or Re-check:</p>
              <CodeBlock
                text={failedProvider.install_command}
                language={installerShellLanguage(failedProvider.install_command)}
                wrapLines
              />
            </div>
          )}
        </div>}
      </AgentProviderPicker>
    </div>
  );
}
