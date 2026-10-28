import { useId, useMemo, useState } from "react";
import type { RuntimeServiceTierSelection } from "../../../packages/plugin-sdk/src/agentRuntime";
import { useAgentProviders } from "../../hooks/useAgentProviders";
import { isProviderSelectable } from "../../lib/providerAvailability";
import { resolveEffort, resolveProviderModel } from "../../lib/newSessionChoice";
import type { ProviderDescriptor } from "../../lib/types";
import { Button } from "../shared/Button";
import { FormDialog } from "../shared/FormDialog";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { OrchestrationRoleRow } from "../Sidebar/OrchestrationPairForm";
import { ServiceTierPicker } from "../Terminal/ServiceTierPicker";
import { focusTab } from "./neighborAgent";
import type { NeighborConciergeClient } from "./neighborConciergeClient";
import type { MachineAccess } from "./neighborRuntimeClient";

/** A provider can answer this friend when it declares concierge support for the friend's machine access. While no
 *  provider declares the capability at all (an older daemon), every provider stays offered and the runtime decides. */
export function conciergeProviders(providers: readonly ProviderDescriptor[], access: MachineAccess | null): ProviderDescriptor[] {
  if (!providers.some(provider => provider.neighbor_concierge)) return [...providers];
  return providers.filter(provider => provider.neighbor_concierge?.supported && (access === null || provider.neighbor_concierge.machine_access.includes(access)));
}

/** Picks the harness, model, effort and speed that answer this Neighbor, with the same agent row as New session; the
 *  friendship keeps its concierge binding. */
export function ChangeConciergeDialog({ friendship, name, currentProvider, machineAccess = null, client, onClose }: {
  friendship: string; name: string; currentProvider: string | null;
  /** The friend's machine access, which the provider must be able to serve. */
  machineAccess?: MachineAccess | null;
  client: NeighborConciergeClient; onClose: () => void;
}) {
  const title = useId();
  const description = useId();
  const { providers: all } = useAgentProviders();
  const providers = useMemo(() => conciergeProviders(all, machineAccess), [all, machineAccess]);
  const unavailable = useMemo(() => new Set(providers.filter(entry => !isProviderSelectable(entry)).map(entry => entry.id)), [providers]);
  const [providerId, setProviderId] = useState(currentProvider ?? "");
  const [model, setModel] = useState<string | undefined>(undefined);
  const [effort, setEffort] = useState<string | undefined>(undefined);
  const [tier, setTier] = useState<RuntimeServiceTierSelection>({ id: null });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const provider = providers.find(entry => entry.id === providerId);
  const modelId = model ?? resolveProviderModel(provider) ?? "";
  const effortId = resolveEffort(provider, modelId, effort);
  const tiers = provider?.agent_runtime?.serviceTiers ?? [];
  // Where this machine cannot confine the friend's access, the folder is a guideline the concierge is asked to keep.
  const advisory = provider?.neighbor_concierge && machineAccess && !provider.neighbor_concierge.enforced_machine_access.includes(machineAccess);
  return <FormDialog titleId={title} descriptionId={description} className="max-w-md" onCancel={() => { if (!busy) onClose(); }} onSubmit={event => {
    event.preventDefault();
    if (!provider || busy || unavailable.has(provider.id)) return;
    setBusy(true); setError(null);
    void client.changeAgent(friendship, provider.id, modelId || undefined, {
      ...(effortId ? { reasoning_effort: effortId } : {}),
      ...(tiers.length ? { service_tier: tier.id ?? "default" } : {}),
    }).then(tabId => { onClose(); focusTab(tabId, null); },
      (failure: unknown) => setError(failure instanceof Error ? failure.message : "The concierge could not be changed.")).finally(() => setBusy(false));
  }}>
    <h2 id={title} className="font-semibold">Change {name}'s concierge</h2>
    <p id={description} className="text-sm text-[var(--ct-muted-fg)]">The agent that reads {name}'s messages and answers within the permissions you set.</p>
    <OrchestrationRoleRow label="Agent" providers={providers} providerId={providerId} modelId={modelId} effortId={effortId} disabledProviderIds={unavailable}
      onProviderChange={id => {
        setProviderId(id);
        setModel(resolveProviderModel(providers.find(entry => entry.id === id)));
        setEffort(undefined);
        setTier({ id: null });
      }}
      onModelChange={next => { setModel(next); setEffort(current => resolveEffort(provider, next, current)); }}
      onEffortChange={setEffort} />
    {tiers.length > 0 && <ServiceTierPicker declared={tiers} available={undefined} selection={tier} busy={busy} onChange={setTier} />}
    {providers.length === 0 && <p role="status" className="text-xs text-[var(--ct-muted-fg)]">No installed agent can be a concierge with this friend's machine access.</p>}
    {advisory && <p className="text-xs text-[var(--ct-muted-fg)]">On this machine its file access is a guideline the agent is asked to keep, not a sandbox.</p>}
    {error && <InlineErrorNotice error={error} operation="changing the concierge" role="alert" />}
    <div className="flex justify-end gap-2">
      <Button size="sm" disabled={busy} onClick={onClose}>Cancel</Button>
      <Button size="sm" variant="primary" type="submit" loading={busy} disabled={!provider || unavailable.has(providerId)}>Change agent</Button>
    </div>
  </FormDialog>;
}
