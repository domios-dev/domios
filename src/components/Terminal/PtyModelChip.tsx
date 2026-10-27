import { useCallback, useState } from "react";
import { useComposerPopover } from "./useComposerPopover";
import type { ProviderDescriptor } from "../../lib/types";
import { reasoningEffortsForModel, resolveEffort } from "../../lib/newSessionChoice";
import { composerPickerState } from "./composerModels";
import { modelSwitchLabel } from "../../lib/modelSwitch";
import { useMuxSelector } from "../../store/multiplexer";
import { getTabModelApi, type ModelSwitchResult } from "../Sidebar/tabModelApi";
import { AnchoredPopover } from "../shared/AnchoredPopover";

export function PtyModelChip({ tabId, provider, onError }: {
  tabId: string;
  provider: ProviderDescriptor | undefined;
  onError: (message: string) => void;
}) {
  const tab = useMuxSelector(useCallback((state) => state.tabs[tabId], [tabId]));
  const { open, triggerRef, popoverRef, close, toggle } = useComposerPopover();
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<ModelSwitchResult | null>(null);
  if (!tab?.pty_id || tab.protected || !provider) return null;
  const models = provider.available_models;
  const picker = composerPickerState(provider.model_switch, provider.supports_permission_bypass);
  const model = tab.agent_provider_resolved_model ?? tab.agent_session_model;
  const efforts = reasoningEffortsForModel(provider, model ?? undefined);
  const requested = tab.reasoning_effort_identity?.resolved ?? tab.reasoning_effort_identity?.requested;
  const effort = requested ?? undefined;

  async function switchModel(model: string, effort?: string) {
    setBusy(true);
    try { setResult((await getTabModelApi().switchModel(tabId, model, effort)) ?? null); }
    catch (error) { onError(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(false); }
  }

  return <div className="relative inline-flex text-xs">
    <button ref={triggerRef} type="button" aria-label="Model and traits" aria-expanded={open} aria-haspopup="dialog"
      className="rounded-full border px-3 py-1" onClick={toggle}>
      {models.find((entry) => entry.id === model)?.display_name ?? model ?? "Model unknown"}{effort ? ` · ${effort}` : ""}
    </button>
    <AnchoredPopover open={open} anchor={triggerRef} placement="top-start" onClose={close}>
    <div ref={popoverRef} role="dialog" aria-label="Model and traits" className="ct-menu"
      style={{ minWidth: 256, padding: 12 }}
      onKeyDown={(event) => { if (event.code === "Escape") { event.preventDefault(); close(); } }}>
      <fieldset role="radiogroup" aria-label="Model" disabled={busy || !picker.modelEnabled}><legend>Model</legend>
        {models.map((entry) => <label key={entry.id} className="flex gap-2 py-1">
          <input type="radio" name={`model-${tabId}`} checked={model === entry.id} disabled={entry.availability === "unavailable"}
            onChange={() => void switchModel(entry.id, resolveEffort(provider, entry.id, effort))} />{entry.display_name}
        </label>)}
      </fieldset>
      {efforts.length > 0 && <fieldset role="radiogroup" aria-label="Reasoning level" disabled={busy || !model || !picker.modelEnabled}><legend>Reasoning level</legend>
        {efforts.map((entry) => <label key={entry.id} className="flex gap-2 py-1">
          <input type="radio" name={`effort-${tabId}`} checked={effort === entry.id}
            onChange={() => { if (model) void switchModel(model, entry.id); }} />{entry.display_name}
        </label>)}
      </fieldset>}
      <span>{modelSwitchLabel(provider.model_switch)}</span>
      {!picker.modelEnabled && <p>This tab does not declare model switching.</p>}
      <button type="button" disabled={!picker.fastEnabled} title="This tab does not declare fast control">Fast unavailable</button>
      <button type="button" disabled title={picker.runtimeMode === "launch_flag" ? "Permissions are a launch flag for this tab" : "This tab does not declare runtime mode control"}>
        {picker.runtimeMode === "launch_flag" ? `Permissions bypass: ${tab.auto_approve === undefined ? "unknown" : tab.auto_approve ? "on" : "off"} (launch flag)` : "Runtime mode unavailable"}
      </button>
      <button type="button" onClick={close}>Close</button>
    </div>
    </AnchoredPopover>
    {result && <span role="status">{modelSwitchLabel(result.mode_used)}{result.model_confirmation === "pending" ? " · awaiting confirmation" : ""}</span>}
  </div>;
}
