import { useEffect, useRef, useState } from "react";
import { invoke, listen, type UnlistenFn } from "../../lib/bridge";
import { classifyTabTarget } from "../../lib/remoteTabTransport";
import { useAgentDescriptors } from "../../lib/useAgentDescriptor";
import { useTabActivity } from "../../lib/useTabActivity";
import { isProviderSelectable } from "../../lib/providerAvailability";
import { describeError } from "../../lib/describeError";
import { reasoningEffortsForModel, resolveEffort, resolveProviderModel } from "../../lib/newSessionChoice";
import type { Tab } from "../../lib/types";
import { getTabProviderApi } from "./tabProviderApi";
import { supportsProviderHandoff, switchTabProvider, type ProviderSwitchResult, type ProviderSwitchStatus } from "./tabProviderMenu";
import type { ProviderSwitchStatusPhase } from "./providerSwitchStatusModel";

function providerSwitchStatusEvent(tabId: string): string {
  return classifyTabTarget(tabId).kind === "remote" ? `provider-switch-status:${tabId}` : "provider-switch-status";
}

export function useProviderSwitch(tab: Tab, initialProviderId: string) {
  const providers = useAgentDescriptors().filter((provider) => supportsProviderHandoff(provider) && provider.id !== tab.agent_type);
  const [providerId, updateProviderId] = useState(initialProviderId);
  const provider = providers.find((candidate) => candidate.id === providerId && isProviderSelectable(candidate));
  const [selectedModel, setModel] = useState<string>();
  const model = resolveProviderModel(provider, selectedModel) ?? selectedModel;
  const [selectedEffort, setEffort] = useState<string>();
  const effort = resolveEffort(provider, model, selectedEffort);
  const efforts = reasoningEffortsForModel(provider, model);
  const activity = useTabActivity(tab.id, tab);
  const [phase, setPhase] = useState<"choosing" | "waiting" | "switching" | "complete">("choosing");
  const [error, setError] = useState<string | null>(null);
  const [retryAfterSeconds, setRetryAfterSeconds] = useState<number | null>(null);
  const [switchStatus, setSwitchStatus] = useState<ProviderSwitchStatusPhase | null>(null);
  const [result, setResult] = useState<ProviderSwitchResult | null>(null);
  const [history, setHistory] = useState<Array<{ kind: string; content: string }> | null>(null);
  const submitting = useRef(false);
  const activeOperationId = useRef<string | null>(null);
  const earlyStatus = useRef<ProviderSwitchStatus | null>(null);
  const statusListenerReady = useRef<Promise<void>>(Promise.resolve());
  const statusHandler = useRef<(status: ProviderSwitchStatus) => void>(() => {});
  const ready = activity.status === "Idle" && !activity.launching;

  function failSwitch(detail: string) {
    activeOperationId.current = null;
    setError(detail);
    setRetryAfterSeconds(describeError(detail, { operation: "switching the harness" }).retryAfterSeconds ?? null);
    setSwitchStatus({ phase: "failed", error: detail });
    setPhase("choosing");
  }

  function applyStatus(status: ProviderSwitchStatus) {
    if (status.phase === "failed") {
      failSwitch(status.error ?? "The destination provider could not be bound.");
      return;
    }
    if (status.phase === "completed") {
      const outcome = status.outcome;
      if (!outcome?.handoff_delivered || !outcome.binding_restored) {
        failSwitch("The provider did not confirm handoff delivery and session binding.");
        return;
      }
      activeOperationId.current = null;
      setSwitchStatus({ phase: "completed", error: null });
      setResult(outcome);
      setError(null);
      setRetryAfterSeconds(null);
      setPhase("complete");
      return;
    }
    setSwitchStatus({ phase: status.phase, error: status.error });
  }

  statusHandler.current = (status) => {
    if (status.tab_id !== tab.id) return;
    if (activeOperationId.current === null) {
      earlyStatus.current = status;
      return;
    }
    if (status.operation_id === activeOperationId.current) applyStatus(status);
  };

  useEffect(() => {
    let active = true;
    let unlisten: UnlistenFn | null = null;
    const ready = listen<ProviderSwitchStatus>(providerSwitchStatusEvent(tab.id), ({ payload }) => {
      if (active) statusHandler.current(payload);
    }).then((stop) => {
      if (active) unlisten = stop;
      else stop();
    });
    statusListenerReady.current = ready;
    void ready.catch(() => undefined);
    return () => {
      active = false;
      unlisten?.();
    };
  }, [tab.id]);

  function setProviderId(id: string) {
    updateProviderId(id);
    setError(null);
    setRetryAfterSeconds(null);
    setSwitchStatus(null);
  }

  async function submit(force = false) {
    if (!provider || submitting.current || retryAfterSeconds !== null) return;
    submitting.current = true;
    activeOperationId.current = null;
    earlyStatus.current = null;
    setSwitchStatus(null);
    setPhase("switching");
    setError(null);
    try {
      await statusListenerReady.current;
      const accepted = await switchTabProvider(getTabProviderApi(), tab.id, provider.id, { model, reasoningEffort: effort, force });
      if (accepted && "operation_id" in accepted) {
        activeOperationId.current = accepted.operation_id;
        const observed = earlyStatus.current;
        earlyStatus.current = null;
        applyStatus(observed?.operation_id === accepted.operation_id ? observed : accepted);
        return;
      }
      if (!accepted || !accepted.handoff_delivered || !accepted.binding_restored) {
        throw new Error("The provider did not confirm handoff delivery and session binding.");
      }
      setSwitchStatus({ phase: "completed", error: null });
      setResult(accepted);
      setPhase("complete");
    } catch (error) {
      failSwitch(error instanceof Error ? error.message : String(error));
    } finally { submitting.current = false; }
  }

  const submitRef = useRef(submit);
  submitRef.current = submit;
  useEffect(() => {
    if (retryAfterSeconds === null) return;
    const timer = setTimeout(() => {
      setRetryAfterSeconds(null);
      setError(null);
      setSwitchStatus(null);
    }, retryAfterSeconds * 1000);
    return () => clearTimeout(timer);
  }, [retryAfterSeconds]);
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
    phase, setPhase, error, retryAfterSeconds, switchStatus, result, history, ready, activity, submit, showHistory };
}
