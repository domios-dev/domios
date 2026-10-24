import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useMemo } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import { AgentProviderPicker } from "./AgentProviderPicker";
import { generalAgentProviders } from "./preselectProvider";
import { usePickerNavigation, type TabRef } from "./pickerNavigation";
import { useFirstRunStart } from "./useFirstRunStart";

export function FirstRunGeneralPicker({ providers, home }: { providers: ProviderDescriptor[]; home?: TabRef }) {
  const eligibleProviders = useMemo(() => generalAgentProviders(providers), [providers]);
  const navigation = usePickerNavigation();

  const { busy, error, onConfirm } = useFirstRunStart(navigation.show);

  return (
    <div className="h-full w-full bg-[var(--ct-bg)]" data-testid="first-run-general-picker">
      <AgentProviderPicker
        headline="Welcome to Domios"
        subhead="Pick the agent that runs your General tab. You can change it later in Settings."
        providers={eligibleProviders}
        confirmLabel="Start General agent"
        busy={busy}
        onConfirm={onConfirm}
        navigation={navigation}
        home={home}
      />
      {error && (
        <div data-testid="first-run-error"><InlineErrorNotice error={error} operation="starting the General agent" /></div>
      )}
    </div>
  );
}
