import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useMemo } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import { AgentProviderPicker } from "./AgentProviderPicker";
import { generalAgentProviders } from "./preselectProvider";
import { usePickerNavigation, type TabRef } from "./pickerNavigation";
import { RuntimeLoginActions } from "../Terminal/RuntimeLoginActions";
import { useFirstRunLogin } from "./useFirstRunLogin";
import { useFirstRunStart } from "./useFirstRunStart";
import { FirstRunSetupProgress } from "./FirstRunSetupProgress";

export function FirstRunGeneralPicker({ providers, home }: { providers: ProviderDescriptor[]; home?: TabRef }) {
  const eligibleProviders = useMemo(() => generalAgentProviders(providers), [providers]);
  const navigation = usePickerNavigation();

  const { login, beforeStart } = useFirstRunLogin(home, eligibleProviders);
  const { busy, phase, error, progress, onConfirm, onInstall } = useFirstRunStart(navigation.show, beforeStart);

  return (
    <div className="h-full w-full bg-[var(--ct-bg)]" data-testid="first-run-general-picker">
      <AgentProviderPicker
        headline="Welcome to Domios"
        subhead="Pick the agent that runs your General tab. You can change it later in Settings."
        providers={eligibleProviders}
        confirmLabel="Start General agent"
        busy={busy}
        retry={Boolean(error)}
        onConfirm={onConfirm}
        onInstall={onInstall}
        progress={login ? "Sign in to continue" : progress}
        navigation={navigation}
        home={home}
      >
        <FirstRunSetupProgress phase={phase} />
        {login && home && <RuntimeLoginActions tabId={home.tabId} providerId={login.providerId} state={login.state} />}
        {error && <div data-testid="first-run-error"><InlineErrorNotice error={error} operation="setting up the General agent" blocking /></div>}
      </AgentProviderPicker>
    </div>
  );
}
