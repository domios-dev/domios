import { useId, useMemo, useState } from "react";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { isProviderSelectable } from "../../lib/providerAvailability";
import { resolveProviderModel } from "../../lib/newSessionChoice";
import { Button } from "../shared/Button";
import { FormDialog } from "../shared/FormDialog";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { OrchestrationRoleRow } from "../Sidebar/OrchestrationPairForm";
import { focusTab } from "./neighborAgent";
import type { NeighborConciergeClient } from "./neighborConciergeClient";

/** Picks the harness and model that answers this Neighbor with the same agent row as New session; the friendship keeps its concierge binding. */
export function ChangeConciergeDialog({ friendship, name, currentProvider, client, onClose }: {
  friendship: string; name: string; currentProvider: string | null; client: NeighborConciergeClient; onClose: () => void;
}) {
  const title = useId();
  const description = useId();
  const { providers } = useAgentProviders();
  const unavailable = useMemo(() => new Set(providers.filter(entry => !isProviderSelectable(entry)).map(entry => entry.id)), [providers]);
  const [providerId, setProviderId] = useState(currentProvider ?? "");
  const [model, setModel] = useState<string | undefined>(undefined);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const provider = providers.find(entry => entry.id === providerId);
  const modelId = model ?? resolveProviderModel(provider) ?? "";
  return <FormDialog titleId={title} descriptionId={description} className="max-w-md" onCancel={() => { if (!busy) onClose(); }} onSubmit={event => {
    event.preventDefault();
    if (!provider || busy || unavailable.has(provider.id)) return;
    setBusy(true); setError(null);
    void client.changeAgent(friendship, provider.id, modelId || undefined).then(tabId => { onClose(); focusTab(tabId, null); },
      (failure: unknown) => setError(failure instanceof Error ? failure.message : "The concierge could not be changed.")).finally(() => setBusy(false));
  }}>
    <h2 id={title} className="font-semibold">Change {name}'s concierge</h2>
    <p id={description} className="text-sm text-[var(--ct-muted-fg)]">The agent that reads {name}'s messages and answers within the permissions you set.</p>
    {/* Effort stays out until change_agent carries it, so the dialog never offers a choice the concierge would ignore. */}
    <OrchestrationRoleRow label="Agent" providers={providers} providerId={providerId} modelId={modelId} disabledProviderIds={unavailable}
      onProviderChange={id => {
        setProviderId(id);
        setModel(resolveProviderModel(providers.find(entry => entry.id === id)));
      }}
      onModelChange={setModel} />
    {error && <InlineErrorNotice error={error} operation="changing the concierge" role="alert" />}
    <div className="flex justify-end gap-2">
      <Button size="sm" disabled={busy} onClick={onClose}>Cancel</Button>
      <Button size="sm" variant="primary" type="submit" loading={busy} disabled={!provider || unavailable.has(providerId)}>Change agent</Button>
    </div>
  </FormDialog>;
}
