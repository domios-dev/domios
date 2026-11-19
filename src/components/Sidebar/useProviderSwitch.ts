import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke, listen, type UnlistenFn } from "../../lib/bridge";
import { EVENT_STREAM_LOST, EVENT_STREAM_READY } from "../../lib/eventStreamReady";
import { onResync } from "../../lib/resync";
import { classifyTabTarget } from "../../lib/remoteTabTransport";
import { onRemoteTabEventSignal } from "../../lib/remoteTabEvents";
import { useAgentDescriptors } from "../../lib/useAgentDescriptor";
import { useTabActivity } from "../../lib/useTabActivity";
import { isProviderSelectable } from "../../lib/providerAvailability";
import { describeError } from "../../lib/describeError";
import { resolveEffort, resolveProviderModel } from "../../lib/newSessionChoice";
import type { ProviderDescriptor, Tab } from "../../lib/types";
import { getTabProviderApi } from "./tabProviderApi";
import { supportsProviderHandoff, switchTabProvider, type ProviderSwitchResult, type ProviderSwitchStatus } from "./tabProviderMenu";
import { preferProviderSwitchStatus, type ProviderSwitchStatusPhase } from "./providerSwitchStatusModel";

function providerSwitchStatusEvent(tabId: string): string {
  return classifyTabTarget(tabId).kind === "remote" ? `provider-switch-status:${tabId}` : "provider-switch-status";
}

type DialogPhase = "choosing" | "waiting" | "switching" | "complete";
type StatusRegistration = () => Promise<void>;
type SwitchTarget = { provider: ProviderDescriptor; model?: string; effort?: string };
const MAX_EARLY_STATUS_OPERATIONS = 128;

export function useProviderSwitch(tab: Tab, initialProviderId: string) {
  const providers = useAgentDescriptors().filter((provider) => supportsProviderHandoff(provider) && provider.id !== tab.agent_type);
  const [providerId, updateProviderId] = useState(initialProviderId);
  const provider = providers.find((candidate) => candidate.id === providerId && isProviderSelectable(candidate));
  const [selectedModel, setModel] = useState<string>();
  const model = resolveProviderModel(provider, selectedModel) ?? selectedModel;
  const [selectedEffort, setEffort] = useState<string>();
  const effort = resolveEffort(provider, model, selectedEffort);
  const activity = useTabActivity(tab.id, tab);
  const [phase, setPhase] = useState<DialogPhase>("choosing");
  const [error, setError] = useState<string | null>(null);
  const [statusUncertainty, setStatusUncertainty] = useState<string | null>(null);
  const [retryAfterSeconds, setRetryAfterSeconds] = useState<number | null>(null);
  const [switchStatus, setSwitchStatus] = useState<ProviderSwitchStatusPhase | null>(null);
  const [submittedTarget, setSubmittedTarget] = useState<SwitchTarget | null>(null);
  const submitting = useRef(false);
  const activeOperationId = useRef<string | null>(null);
  const acceptedStatus = useRef<ProviderSwitchStatus | null>(null);
  const earlyStatuses = useRef(new Map<string, ProviderSwitchStatus>());
  const statusRegistration = useRef<StatusRegistration | null>(null);
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

  function stopObserving() {
    statusRegistration.current = null;
    submitting.current = false;
    clearAttempt();
    reconciliationRequests.current.clear();
    reconciliationAgain.current.clear();
  }

  function failSwitch(detail: string) {
    clearAttempt();
    setError(detail);
    setStatusUncertainty(null);
    setRetryAfterSeconds(describeError(detail, { operation: "switching providers" }).retryAfterSeconds ?? null);
    setSwitchStatus({ phase: "failed", error: detail });
    setPhase("choosing");
  }

  function applyStatus(status: ProviderSwitchStatus) {
    if (status.operation_id !== activeOperationId.current || status.tab_id !== tab.id) return;
    setStatusUncertainty(null);
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
      setError(null);
      setRetryAfterSeconds(null);
      setPhase("complete");
      return;
    }
    setSwitchStatus({ phase: preferred.phase, error: preferred.error });
    setError(null);
    setPhase("switching");
  }

  function reconcileAcceptedStatus(operationId: string, registration: StatusRegistration): Promise<void> {
    const isCurrent = () => statusRegistration.current === registration && activeOperationId.current === operationId;
    if (!isCurrent()) return Promise.resolve();
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
        if (!isCurrent()) return;
        if (!status || status.operation_id !== operationId) {
          throw new Error("The provider switch status did not match the accepted operation.");
        }
        applyStatus({ ...status, tab_id: tab.id });
      } catch (failure) {
        if (isCurrent()) setStatusUncertainty(failure instanceof Error ? failure.message : String(failure));
      } finally {
        if (statusRegistration.current === registration) {
          reconciliationRequests.current.delete(operationId);
          if (reconciliationAgain.current.delete(operationId) && isCurrent()) {
            void reconcileAcceptedStatus(operationId, registration);
          }
        }
      }
    })();
    reconciliationRequests.current.set(operationId, request);
    return request;
  }

  statusHandler.current = (status) => {
    if (status.tab_id !== tab.id) return;
    if (activeOperationId.current === status.operation_id) {
      applyStatus(status);
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

  useLayoutEffect(() => {
    let unlisten: UnlistenFn | null = null;
    let registering: Promise<void> | null = null;
    const isCurrent = () => statusRegistration.current === ensureStatusListener;
    const ensureStatusListener = (): Promise<void> => {
      if (!isCurrent()) return Promise.reject(new Error("The provider status listener is no longer active."));
      if (unlisten) return Promise.resolve();
      if (registering) return registering;
      const attempt = listen<ProviderSwitchStatus>(statusEvent, ({ payload }) => {
        if (isCurrent()) statusHandler.current(payload);
      }).then((stop) => {
        if (isCurrent()) unlisten = stop;
        else stop();
      });
      registering = attempt.catch((failure) => {
        registering = null;
        throw failure;
      });
      return registering;
    };
    const refresh = () => {
      const operationId = activeOperationId.current;
      if (operationId) void reconcileAcceptedStatus(operationId, ensureStatusListener);
    };
    statusRegistration.current = ensureStatusListener;
    let stopSignals: () => void;
    if (remoteStatusTarget) {
      stopSignals = onRemoteTabEventSignal(statusEvent, refresh);
    } else {
      window.addEventListener(EVENT_STREAM_READY, refresh);
      window.addEventListener(EVENT_STREAM_LOST, refresh);
      const stopResync = onResync(refresh);
      stopSignals = () => {
        window.removeEventListener(EVENT_STREAM_READY, refresh);
        window.removeEventListener(EVENT_STREAM_LOST, refresh);
        stopResync();
      };
    }
    void ensureStatusListener().catch(() => undefined);
    return () => {
      if (isCurrent()) stopObserving();
      unlisten?.();
      stopSignals();
    };
  }, [statusEvent, remoteStatusTarget, tab.id]);

  function setProviderId(id: string) {
    updateProviderId(id);
    setError(null);
    setStatusUncertainty(null);
    setSwitchStatus(null);
  }

  async function submit(force = false) {
    const registration = statusRegistration.current;
    if (!registration || !provider || submitting.current || activeOperationId.current !== null || retryAfterSeconds !== null) return;
    submitting.current = true;
    clearAttempt();
    setSwitchStatus(null);
    setStatusUncertainty(null);
    setSubmittedTarget({ provider, model, effort });
    setPhase("switching");
    setError(null);
    let accepted: ProviderSwitchStatus | ProviderSwitchResult | void;
    try {
      await registration();
      if (statusRegistration.current !== registration) return;
      accepted = await switchTabProvider(getTabProviderApi(), tab.id, provider.id, { model, reasoningEffort: effort, force });
    } catch (failure) {
      if (statusRegistration.current !== registration) return;
      submitting.current = false;
      failSwitch(failure instanceof Error ? failure.message : String(failure));
      return;
    }
    if (statusRegistration.current !== registration) return;
    const early = accepted && "operation_id" in accepted ? earlyStatuses.current.get(accepted.operation_id) : undefined;
    earlyStatuses.current.clear();
    submitting.current = false;
    if (accepted && "operation_id" in accepted) {
      activeOperationId.current = accepted.operation_id;
      const initial = { ...accepted, tab_id: tab.id };
      applyStatus(early ? preferProviderSwitchStatus(initial, early) : initial);
      await reconcileAcceptedStatus(accepted.operation_id, registration);
      return;
    }
    if (!accepted || !accepted.handoff_delivered || !accepted.binding_restored) {
      failSwitch("The provider did not confirm handoff delivery and session binding.");
      return;
    }
    setSwitchStatus({ phase: "completed", error: null });
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

  return { providers, providerId, setProviderId, provider, selectedModel, setModel, model, setEffort, effort,
    phase, setPhase, error, statusUncertainty, retryAfterSeconds, switchStatus, submittedTarget, ready, activity, submit, stopObserving };
}
