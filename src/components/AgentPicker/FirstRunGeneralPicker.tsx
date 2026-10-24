import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useMemo, useState } from "react";
import { invoke } from "../../lib/bridge";
import type { MultiplexerState, ProviderDescriptor } from "../../lib/types";
import { firstRunGeneralTarget } from "./firstRunNavigation";
import { AgentProviderPicker } from "./AgentProviderPicker";
import { generalAgentProviders } from "./preselectProvider";
import { usePickerNavigation, type TabRef } from "./pickerNavigation";

export function FirstRunGeneralPicker({ providers, home }: { providers: ProviderDescriptor[]; home?: TabRef }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const eligibleProviders = useMemo(() => generalAgentProviders(providers), [providers]);
  const navigation = usePickerNavigation();

  const onConfirm = async (providerId: string) => {
    setBusy(true);
    setError(null);
    try {
      await invoke("complete_general_agent_first_run", { providerId, fullAccess: true });
      const state = await invoke<MultiplexerState>("mux_get_state");
      const target = firstRunGeneralTarget(state);
      if (target) await navigation.show(target);
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to start General agent");
      setBusy(false);
    }
  };

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
