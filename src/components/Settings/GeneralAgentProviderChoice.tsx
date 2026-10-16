import { useRef, useState } from "react";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { ProviderCard } from "../AgentPicker/ProviderCard";
import { ConfirmDialog } from "../Extensions/ConfirmDialog";
import { InlineErrorNotice } from "../shared/ErrorNotice";

export function GeneralAgentProviderChoice({ current, enabled, onSwitch }: {
  current: string | undefined;
  enabled: boolean;
  onSwitch: (provider: string) => Promise<void>;
}) {
  const { providers } = useAgentProviders();
  const [pending, setPending] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const applying = useRef(false);
  async function confirm() {
    if (!pending || applying.current) return;
    applying.current = true;
    setBusy(true);
    setError(null);
    try { await onSwitch(pending); setPending(null); }
    catch (cause) { setError(cause); }
    finally { applying.current = false; setBusy(false); }
  }
  return <div className="space-y-3" role="group" aria-label="General Agent provider">
    <div className="text-xs text-[var(--ct-muted-fg)]">General Agent</div>
    <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
      {providers.map((provider) => <ProviderCard key={provider.id} provider={provider} selected={provider.id === current}
        onSelect={() => { if (!busy && provider.id !== current) { setError(null); setPending(provider.id); } }} />)}
    </div>
    {error != null && <InlineErrorNotice error={error} operation="changing the General Agent" />}
    {pending && <ConfirmDialog title="Change General Agent?"
      message={enabled ? "Your General Agent will restart with the selected provider." : "The selected provider will be used when you enable the General Agent."}
      confirmLabel={busy ? "Applying…" : enabled ? "Change and restart" : "Change provider"}
      onConfirm={() => void confirm()} onCancel={() => { if (!busy) setPending(null); }} />}
  </div>;
}
