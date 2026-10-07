import { useCallback, useState } from "react";
import type { ProviderDescriptor } from "../../lib/types";
import { reasoningEffortsForModel, resolveEffort } from "../../lib/newSessionChoice";
import { supportsLiveModelSwitch } from "../../lib/modelSwitch";
import { useMuxSelector } from "../../store/multiplexer";
import { getTabModelApi, type ModelSwitchResult } from "../Sidebar/tabModelApi";

export function PtyModelChip({ tabId, provider, onError }: {
  tabId: string;
  provider: ProviderDescriptor | undefined;
  onError: (message: string) => void;
}) {
  const tab = useMuxSelector(useCallback((state) => state.tabs[tabId], [tabId]));
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<ModelSwitchResult | null>(null);
  if (!tab?.pty_id || tab.protected || !provider || !supportsLiveModelSwitch(provider)) return null;
  const models = provider.available_models;
  if (!models.length) return null;
  const model = tab.agent_provider_resolved_model ?? tab.agent_session_model ?? provider.default_model ?? models[0].id;
  const efforts = reasoningEffortsForModel(provider, model);
  const requested = tab.reasoning_effort_identity?.resolved ?? tab.reasoning_effort_identity?.requested;
  const effort = resolveEffort(provider, model, requested ?? undefined);

  async function switchModel(model: string, effort?: string) {
    setBusy(true);
    try { setResult((await getTabModelApi().switchModel(tabId, model, effort)) ?? null); }
    catch (error) { onError(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(false); }
  }

  return <div role="group" aria-label="Session model" className="inline-flex items-center gap-1 text-xs">
    <label>Model<select aria-label="Session model" disabled={busy} value={model}
      onChange={(event) => void switchModel(event.target.value, resolveEffort(provider, event.target.value, effort))}>
      {models.map((model) => <option key={model.id} value={model.id} disabled={model.availability === "unavailable"}>{model.display_name}</option>)}
    </select></label>
    {efforts.length > 0 && <label>Reasoning level<select aria-label="Reasoning level" disabled={busy} value={effort}
      onChange={(event) => void switchModel(model, event.target.value)}>
      {efforts.map((effort) => <option key={effort.id} value={effort.id}>{effort.display_name}</option>)}
    </select></label>}
    {result && <span role="status">{result.mode_used === "live" ? "Live" : "Restart/resume"}{result.model_confirmation === "pending" ? " · awaiting confirmation" : ""}</span>}
  </div>;
}
