import { useRef, useState } from "react";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { ProviderCard } from "../AgentPicker/ProviderCard";
import { generalAgentProviders } from "../AgentPicker/preselectProvider";
import { ConfirmDialog } from "../Extensions/ConfirmDialog";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import type { HelpSectionId } from "../../help/types";
import { GuideInfoButton } from "./GuideInfoButton";

export function GeneralAgentProviderChoice({ current, enabled, onSwitch, onOpenGuide }: {
  current: string | undefined;
  enabled: boolean;
  onSwitch: (provider: string) => Promise<void>;
  onOpenGuide?: (section: HelpSectionId) => void;
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
    <div className="flex items-center gap-1 text-sm font-medium text-[var(--ct-fg)]">General Agent
      {onOpenGuide && <GuideInfoButton label="General Agent" section="agents" onOpen={onOpenGuide} />}
    </div>
    <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
      {generalAgentProviders(providers).map((provider) => <ProviderCard key={provider.id} provider={provider} selected={provider.id === current}
        onSelect={() => { if (!busy && provider.id !== current) { setError(null); setPending(provider.id); } }} />)}
    </div>
    {error != null && <InlineErrorNotice error={error} operation="changing the General Agent" />}
    {pending && <ConfirmDialog title="Change General Agent?"
      message={enabled ? "Your General Agent will restart with the selected provider." : "The selected provider will be used when you enable the General Agent."}
      confirmLabel={busy ? "Applying…" : enabled ? "Change and restart" : "Change provider"}
      onConfirm={() => void confirm()} onCancel={() => { if (!busy) setPending(null); }} />}
  </div>;
}
