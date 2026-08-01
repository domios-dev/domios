import { useState } from "react";
import { invoke } from "../../lib/bridge";
import type { ProviderDescriptor } from "../../lib/types";
import { AgentProviderPicker } from "./AgentProviderPicker";
import { generalAgentProviders } from "./preselectProvider";

export function FirstRunGeneralPicker({ providers }: { providers: ProviderDescriptor[] }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const eligibleProviders = generalAgentProviders(providers);

  const onConfirm = async (providerId: string) => {
    setBusy(true);
    setError(null);
    try {
      await invoke("complete_general_agent_first_run", { providerId });
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to start General agent");
      setBusy(false);
    }
  };

  return (
    <div className="h-full w-full bg-[var(--ct-bg)]" data-testid="first-run-general-picker">
      <AgentProviderPicker
        headline="Choose your General agent"
        providers={eligibleProviders}
        confirmLabel="Start General agent"
        busy={busy}
        onConfirm={onConfirm}
      />
      {error && (
        <p className="text-center text-[10px] text-[var(--ct-red)] pb-4" data-testid="first-run-error">
          {error}
        </p>
      )}
    </div>
  );
}
