import { useState } from "react";
import { invoke } from "../../lib/bridge";
import type { ProviderDescriptor } from "../../lib/types";
import { AgentProviderPicker } from "./AgentProviderPicker";
import { generalAgentProviders } from "./preselectProvider";

export function FirstRunGeneralPicker({ providers }: { providers: ProviderDescriptor[] }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fullAccess, setFullAccess] = useState(true);
  const eligibleProviders = generalAgentProviders(providers);

  const onConfirm = async (providerId: string) => {
    setBusy(true);
    setError(null);
    try {
      await invoke("complete_general_agent_first_run", { providerId, fullAccess });
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
      >
        <label className="mx-auto flex max-w-md cursor-pointer items-start gap-2.5 rounded-md border border-[var(--ct-border)] bg-[var(--ct-surface)] px-3 py-2.5 text-left">
          <input
            type="checkbox"
            checked={fullAccess}
            onChange={(event) => setFullAccess(event.target.checked)}
            className="mt-0.5 h-3.5 w-3.5 accent-[var(--ct-accent)]"
          />
          <span>
            <span className="block text-[12px] text-[var(--ct-fg)]">Full access for agents</span>
            <span className="mt-0.5 block text-[11px] text-[var(--ct-muted-fg)]">
              Agents can edit files and run commands without asking. Enabled by default.
            </span>
          </span>
        </label>
      </AgentProviderPicker>
      {error && (
        <p className="text-center text-[10px] text-[var(--ct-red)] pb-4" data-testid="first-run-error">
          {error}
        </p>
      )}
    </div>
  );
}
