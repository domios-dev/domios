import { useEffect, useRef, useState } from "react";
import { invoke, listen, type UnlistenFn } from "../../lib/bridge";
import { EVENT_STREAM_LOST, EVENT_STREAM_READY } from "../../lib/eventStreamReady";
import { onResync } from "../../lib/resync";
import { classifyTabTarget } from "../../lib/remoteTabTransport";
import { onRemoteTabEventSignal } from "../../lib/remoteTabEvents";
import { useAgentDescriptors } from "../../lib/useAgentDescriptor";
import { useTabActivity } from "../../lib/useTabActivity";
import { isProviderSelectable } from "../../lib/providerAvailability";
import { describeError } from "../../lib/describeError";
import { reasoningEffortsForModel, resolveEffort, resolveProviderModel } from "../../lib/newSessionChoice";
import type { Tab } from "../../lib/types";
import { getTabProviderApi } from "./tabProviderApi";
import { supportsProviderHandoff, switchTabProvider, type ProviderSwitchResult, type ProviderSwitchStatus } from "./tabProviderMenu";
import { preferProviderSwitchStatus, type ProviderSwitchStatusPhase } from "./providerSwitchStatusModel";

function providerSwitchStatusEvent(tabId: string): string {
  return classifyTabTarget(tabId).kind === "remote" ? `provider-switch-status:${tabId}` : "provider-switch-status";
}

type DialogPhase = "choosing" | "waiting" | "switching" | "complete";
const MAX_EARLY_STATUS_OPERATIONS = 128;

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
  const [phase, setPhase] = useState<DialogPhase>("choosing");
  const [error, setError] = useState<string | null>(null);
  const [statusUncertainty, setStatusUncertainty] = useState<string | null>(null);
  const [observationLost, setObservationLost] = useState(false);
  const [retryAfterSeconds, setRetryAfterSeconds] = useState<number | null>(null);
  const [switchStatus, setSwitchStatus] = useState<ProviderSwitchStatusPhase | null>(null);
  const [result, setResult] = useState<ProviderSwitchResult | null>(null);
  const [history, setHistory] = useState<Array<{ kind: string; content: string }> | null>(null);
  const submitting = useRef(false);
  const activeOperationId = useRef<string | null>(null);
  const acceptedStatus = useRef<ProviderSwitchStatus | null>(null);
  const earlyStatuses = useRef(new Map<string, ProviderSwitchStatus>());
  const statusRegistration = useRef<() => Promise<void>>(() => Promise.resolve());
  const statusHandler = useRef<(status: ProviderSwitchStatus) => void>(() => {});
  const reconciliationRequests = useRef(new Map<string, Promise<void>>());
  const reconciliationAgain = useRef(new Set<string>());
  const ready = activity.status === "Idle" && !activity.launching;
  const statusEvent = providerSwitchStatusEvent(tab.id);
  const remoteStatusTarget = classifyTabTarget(tab.id).kind === "remote";

  function clearAttempt() {
    activeOperationId.current = null;
    acceptedStatus.current = null;
    earlyStatuses.current.clear();
  }

  function failSwitch(detail: string) {
    clearAttempt();
    setError(detail);
    setStatusUncertainty(null);
    setObservationLost(false);
    setRetryAfterSeconds(describeError(detail, { operation: "switching the harness" }).retryAfterSeconds ?? null);
    setSwitchStatus({ phase: "failed", error: detail });
    setPhase("choosing");
  }

  function applyStatus(status: ProviderSwitchStatus, source: "accepted" | "snapshot" | "event") {
    if (status.operation_id !== activeOperationId.current || status.tab_id !== tab.id) return;
    if (source !== "accepted") setStatusUncertainty(null);
    if (source === "event") setObservationLost(false);
    const current = acceptedStatus.current;
    const preferred = preferProviderSwitchStatus(current ?? undefined, status);
    if (current && preferred === current) return;
    acceptedStatus.current = preferred;
    if (preferred.phase === "failed") {
      failSwitch(preferred.error ?? "The destination provider could not be bound.");
      return;
    }
    if (preferred.phase === "completed") {
      const outcome = preferred.outcome;
      if (!outcome?.handoff_delivered || !outcome.binding_restored) {
        failSwitch("The provider did not confirm handoff delivery and session binding.");
        return;
      }
      clearAttempt();
      setSwitchStatus({ phase: "completed", error: null });
      setResult(outcome);
      setError(null);
      setStatusUncertainty(null);
      setObservationLost(false);
      setRetryAfterSeconds(null);
      setPhase("complete");
      return;
    }
    setSwitchStatus({ phase: preferred.phase, error: preferred.error });
    setError(null);
    setPhase("switching");
  }

  function markUnconfirmed(operationId: string, detail: string) {
    if (activeOperationId.current !== operationId) return;
    setStatusUncertainty(detail);
  }

  function reconcileAcceptedStatus(operationId: string): Promise<void> {
    if (activeOperationId.current !== operationId) return Promise.resolve();
    const current = reconciliationRequests.current.get(operationId);
    if (current) {
      reconciliationAgain.current.add(operationId);
      return current;
    }
    const request = (async () => {
      try {
        const status = await invoke<ProviderSwitchStatus | null>("provider_switch_status", {
          tabId: tab.id,
          operationId,
          wait: false,
        });
        if (activeOperationId.current !== operationId) return;
        if (!status || status.operation_id !== operationId) {
          throw new Error("The provider switch status did not match the accepted operation.");
        }
        applyStatus({ ...status, tab_id: tab.id }, "snapshot");
      } catch (failure) {
        markUnconfirmed(operationId, failure instanceof Error ? failure.message : String(failure));
      } finally {
        reconciliationRequests.current.delete(operationId);
        if (reconciliationAgain.current.delete(operationId) && activeOperationId.current === operationId) {
          void reconcileAcceptedStatus(operationId);
        }
      }
    })();
    reconciliationRequests.current.set(operationId, request);
    return request;
  }

  statusHandler.current = (status) => {
    if (status.tab_id !== tab.id) return;
    if (activeOperationId.current === status.operation_id) {
      applyStatus(status, "event");
      return;
    }
    if (!submitting.current) return;
    const previous = earlyStatuses.current.get(status.operation_id);
    if (!previous && earlyStatuses.current.size >= MAX_EARLY_STATUS_OPERATIONS) {
      const oldest = earlyStatuses.current.keys().next();
      if (!oldest.done) earlyStatuses.current.delete(oldest.value);
    }
    earlyStatuses.current.set(status.operation_id, preferProviderSwitchStatus(previous, status));
  };

  useEffect(() => {
    let active = true;
    let unlisten: UnlistenFn | null = null;
    let registration: Promise<void> | null = null;
    const ensureStatusListener = (): Promise<void> => {
      if (!active) return Promise.reject(new Error("The provider status listener is no longer active."));
      if (unlisten) return Promise.resolve();
      if (registration) return registration;
      const attempt = listen<ProviderSwitchStatus>(statusEvent, ({ payload }) => {
        if (active) statusHandler.current(payload);
      }).then((stop) => {
        if (active) unlisten = stop;
        else stop();
      });
      registration = attempt.catch((failure) => {
        registration = null;
        throw failure;
      });
      return registration;
    };
    const reconcileAfterSignal = (lost: boolean) => {
      setObservationLost(lost);
      const operationId = activeOperationId.current;
      if (operationId) void reconcileAcceptedStatus(operationId);
    };
    statusRegistration.current = ensureStatusListener;
    const stopRemoteSignals = remoteStatusTarget
      ? onRemoteTabEventSignal(statusEvent, (signal) => reconcileAfterSignal(signal !== "ready"))
      : null;
    if (!remoteStatusTarget) {
      const onReady = () => reconcileAfterSignal(false);
      const onLost = () => reconcileAfterSignal(true);
      window.addEventListener(EVENT_STREAM_READY, onReady);
      window.addEventListener(EVENT_STREAM_LOST, onLost);
      const stopResync = onResync(() => reconcileAfterSignal(true));
      void ensureStatusListener().catch(() => undefined);
      return () => {
        active = false;
        unlisten?.();
        stopRemoteSignals?.();
        window.removeEventListener(EVENT_STREAM_READY, onReady);
        window.removeEventListener(EVENT_STREAM_LOST, onLost);
        stopResync();
        statusRegistration.current = () => Promise.resolve();
      };
    }
    void ensureStatusListener().catch(() => undefined);
    return () => {
      active = false;
      unlisten?.();
      stopRemoteSignals?.();
      statusRegistration.current = () => Promise.resolve();
    };
  }, [statusEvent, remoteStatusTarget, tab.id]);

  function setProviderId(id: string) {
    updateProviderId(id);
    setError(null);
    setStatusUncertainty(null);
    setSwitchStatus(null);
  }

  async function submit(force = false) {
    if (!provider || submitting.current || activeOperationId.current !== null || retryAfterSeconds !== null) return;
    submitting.current = true;
    clearAttempt();
    setSwitchStatus(null);
    setStatusUncertainty(null);
    setPhase("switching");
    setError(null);
    let accepted: ProviderSwitchStatus | ProviderSwitchResult | void;
    try {
      await statusRegistration.current();
      accepted = await switchTabProvider(getTabProviderApi(), tab.id, provider.id, { model, reasoningEffort: effort, force });
    } catch (failure) {
      earlyStatuses.current.clear();
      submitting.current = false;
      failSwitch(failure instanceof Error ? failure.message : String(failure));
      return;
    }
    const early = accepted && "operation_id" in accepted ? earlyStatuses.current.get(accepted.operation_id) : undefined;
    earlyStatuses.current.clear();
    submitting.current = false;
    setStatusUncertainty(null);
    if (accepted && "operation_id" in accepted) {
      activeOperationId.current = accepted.operation_id;
      const initial = { ...accepted, tab_id: tab.id };
      const observed = early ? { ...early, tab_id: tab.id } : null;
      const preferred = observed ? preferProviderSwitchStatus(initial, observed) : initial;
      applyStatus(preferred, observed && preferred === observed ? "event" : "accepted");
      if (activeOperationId.current === accepted.operation_id) await reconcileAcceptedStatus(accepted.operation_id);
      return;
    }
    if (!accepted || !accepted.handoff_delivered || !accepted.binding_restored) {
      failSwitch("The provider did not confirm handoff delivery and session binding.");
      return;
    }
    setSwitchStatus({ phase: "completed", error: null });
    setResult(accepted);
    setObservationLost(false);
    setPhase("complete");
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
    } catch (failure) { setError(String(failure)); }
  }

  return { providers, providerId, setProviderId, provider, selectedModel, setModel, model, setEffort, effort, efforts,
    phase, setPhase, error, statusUncertainty, observationLost, retryAfterSeconds, switchStatus, result, history, ready, activity, submit, showHistory };
}
