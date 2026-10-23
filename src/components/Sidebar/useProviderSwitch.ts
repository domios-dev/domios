import { useEffect, useRef, useState } from "react";
import { invoke } from "../../lib/bridge";
import { useAgentDescriptors } from "../../lib/useAgentDescriptor";
import { useTabActivity } from "../../lib/useTabActivity";
import { isProviderSelectable } from "../../lib/providerAvailability";
import { reasoningEffortsForModel, resolveEffort, resolveProviderModel } from "../../lib/newSessionChoice";
import type { Tab } from "../../lib/types";
import { getTabProviderApi } from "./tabProviderApi";
import { supportsProviderHandoff, switchTabProvider, type ProviderSwitchResult, type ProviderSwitchStatus } from "./tabProviderMenu";

export function useProviderSwitch(tab: Tab, initialProviderId: string) {
  const providers = useAgentDescriptors().filter((provider) => supportsProviderHandoff(provider) && provider.id !== tab.agent_type);
  const [providerId, setProviderId] = useState(initialProviderId);
  const provider = providers.find((candidate) => candidate.id === providerId && isProviderSelectable(candidate));
  const [selectedModel, setModel] = useState<string>();
  const model = resolveProviderModel(provider, selectedModel) ?? selectedModel;
  const [selectedEffort, setEffort] = useState<string>();
  const effort = resolveEffort(provider, model, selectedEffort);
  const efforts = reasoningEffortsForModel(provider, model);
  const activity = useTabActivity(tab.id, tab);
  const [phase, setPhase] = useState<"choosing" | "waiting" | "switching" | "complete">("choosing");
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<ProviderSwitchResult | null>(null);
  const [history, setHistory] = useState<Array<{ kind: string; content: string }> | null>(null);
  const submitting = useRef(false);
  const ready = activity.status === "Idle" && !activity.launching;

  async function submit(force = false) {
    if (!provider || submitting.current) return;
    submitting.current = true;
    setPhase("switching");
    setError(null);
    try {
      let result = await switchTabProvider(getTabProviderApi(), tab.id, provider.id, { model, reasoningEffort: effort, force });
      if (result && "operation_id" in result) {
        const status = await invoke<ProviderSwitchStatus>("provider_switch_status", { tabId: tab.id, operationId: result.operation_id, wait: true });
        if (status.phase !== "completed") throw new Error(status.error ?? `Switch operation ${status.operation_id} is ${status.phase}. Read its status before trying again.`);
        result = status.outcome ?? undefined;
      }
      if (!result?.handoff_delivered || !result.binding_restored) throw new Error("The provider did not confirm handoff delivery and session binding.");
      setResult(result);
      setPhase("complete");
    } catch (error) {
      setError(String(error));
      setPhase("choosing");
    } finally { submitting.current = false; }
  }

  const submitRef = useRef(submit);
  submitRef.current = submit;
  useEffect(() => {
    if (phase === "waiting" && ready) void submitRef.current();
  }, [phase, ready]);

  async function showHistory() {
    if (!result) return;
    try {
      const rows: unknown = await invoke("read_provider_history", { tabId: tab.id, file: result.previous_history_path });
      if (!Array.isArray(rows)) throw new Error("The previous transcript is unavailable.");
      setHistory(rows.filter((row): row is { type: string; content: string } => row !== null && typeof row === "object" && typeof row.type === "string" && typeof row.content === "string").map((row) => ({ kind: row.type, content: row.content })));
    } catch (error) { setError(String(error)); }
  }

  return { providers, providerId, setProviderId, provider, selectedModel, setModel, model, setEffort, effort, efforts,
    phase, setPhase, error, result, history, ready, activity, submit, showHistory };
}
