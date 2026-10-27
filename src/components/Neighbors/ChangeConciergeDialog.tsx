import { useId, useState } from "react";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { isProviderSelectable, providerAvailabilityLabel } from "../../lib/providerAvailability";
import { Button } from "../shared/Button";
import { DropdownSelect } from "../shared/DropdownMenu";
import { FormDialog } from "../shared/FormDialog";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { focusTab } from "./neighborAgent";
import type { NeighborConciergeClient } from "./neighborConciergeClient";

/** Picks the harness and model that answers this Neighbor; the friendship keeps its concierge binding. */
export function ChangeConciergeDialog({ friendship, name, currentProvider, client, onClose }: {
  friendship: string; name: string; currentProvider: string | null; client: NeighborConciergeClient; onClose: () => void;
}) {
  const title = useId();
  const description = useId();
  const { providers } = useAgentProviders();
  const [providerId, setProviderId] = useState(currentProvider ?? "");
  const [model, setModel] = useState<string | undefined>(undefined);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const provider = providers.find(entry => entry.id === providerId);
  return <FormDialog titleId={title} descriptionId={description} className="max-w-md" onCancel={() => { if (!busy) onClose(); }} onSubmit={event => {
    event.preventDefault();
    if (!provider || busy) return;
    setBusy(true); setError(null);
    void client.changeAgent(friendship, provider.id, model).then(tabId => { onClose(); focusTab(tabId, null); },
      (failure: unknown) => setError(failure instanceof Error ? failure.message : "The concierge could not be changed.")).finally(() => setBusy(false));
  }}>
    <h2 id={title} className="font-semibold">Change {name}'s concierge</h2>
    <p id={description} className="text-sm text-[var(--ct-muted-fg)]">The agent that reads {name}'s messages and answers within the permissions you set.</p>
    <DropdownSelect aria-label="Agent" value={providerId} disabled={busy}
      options={providers.map(entry => ({ value: entry.id, label: isProviderSelectable(entry) ? entry.display_name : `${entry.display_name} (${providerAvailabilityLabel(entry)})`, disabled: !isProviderSelectable(entry) }))}
      onChange={id => { setProviderId(id); setModel(undefined); }} />
    {(provider?.available_models.length ?? 0) > 0 && <DropdownSelect aria-label="Model" value={model ?? ""} disabled={busy}
      options={(provider?.available_models ?? []).map(entry => ({ value: entry.id, label: entry.display_name, disabled: entry.available === false }))}
      onChange={setModel} />}
    {error && <InlineErrorNotice error={error} operation="changing the concierge" role="alert" />}
    <div className="flex justify-end gap-2">
      <Button size="sm" disabled={busy} onClick={onClose}>Cancel</Button>
      <Button size="sm" variant="primary" type="submit" loading={busy} disabled={!provider}>Change agent</Button>
    </div>
  </FormDialog>;
}
