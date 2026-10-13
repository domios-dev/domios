import { useEffect, useRef, useState } from "react";
import { invoke, listen } from "../../lib/bridge";
import {
  beginRuntimeRetry, runtimeRetryFailed, runtimeStateFromEvent, runtimeStateFromSnapshot,
  type AgentRuntimeState, type AgentRuntimeStateEvent,
} from "../../lib/agentRuntimeState";

export function useAgentRuntimeState(tabId: string, nonce: string | null, snapshot?: AgentRuntimeState | null) {
  const key = JSON.stringify([tabId, nonce]);
  const [record, setRecord] = useState(() => ({ key, state: runtimeStateFromSnapshot(null, snapshot) }));
  const current = useRef(record);
  const revision = useRef(0);
  const generation = useRef(0);
  const snapshotState = snapshot?.state;
  const snapshotEpoch = snapshot?.epoch;
  const snapshotReason = snapshot?.reason;
  const pending = useRef(false);
  const publish = (state: AgentRuntimeState | null) => {
    current.current = { key, state };
    setRecord(current.current);
  };

  useEffect(() => {
    revision.current++;
    const next = snapshotState !== undefined && snapshotEpoch !== undefined
      ? { state: snapshotState, epoch: snapshotEpoch, reason: snapshotReason } : null;
    const state = runtimeStateFromSnapshot(current.current.key === key ? current.current.state : null, next);
    if (current.current.key === key && current.current.state === state) return;
    current.current = { key, state };
    setRecord(current.current);
  }, [key, snapshotState, snapshotEpoch, snapshotReason]);

  useEffect(() => {
    generation.current++;
    pending.current = false;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const eventName = tabId.startsWith("mesh:") ? `agent-runtime-state:${tabId}` : "agent-runtime-state";
    void listen<AgentRuntimeStateEvent>(eventName, ({ payload }) => {
      if (disposed) return;
      const previous = current.current.key === key ? current.current.state : null;
      const state = runtimeStateFromEvent(previous, { tabId, nonce }, payload);
      if (state === previous) return;
      revision.current++;
      current.current = { key, state };
      setRecord(current.current);
    }).then((stop) => { if (disposed) stop(); else unlisten = stop; }).catch((error: unknown) => {
      if (disposed || !current.current.state) return;
      revision.current++;
      const state = { ...current.current.state, state: "degraded" as const,
        reason: error instanceof Error ? error.message : String(error) };
      current.current = { key, state };
      setRecord(current.current);
    });
    return () => { disposed = true; generation.current++; revision.current++; unlisten?.(); };
  }, [key, tabId, nonce]);

  const retry = async () => {
    if (pending.current || current.current.key !== key) return;
    const state = beginRuntimeRetry(current.current.state);
    if (state === current.current.state) return;
    pending.current = true;
    publish(state);
    const startedRevision = revision.current;
    const startedGeneration = generation.current;
    const ownsRequest = () => current.current.key === key && generation.current === startedGeneration;
    const canSettle = () => ownsRequest() && (revision.current === startedRevision || current.current.state?.state === "loading");
    try {
      const result = await invoke<AgentRuntimeState>("agent_runtime_retry", { tabId });
      if (canSettle()) {
        publish(runtimeStateFromSnapshot(current.current.state, result));
      }
    } catch (error) {
      if (canSettle()) {
        publish(runtimeRetryFailed(current.current.state, error instanceof Error ? error.message : String(error)));
      }
    } finally {
      if (ownsRequest()) pending.current = false;
    }
  };

  return { state: record.key === key ? record.state : runtimeStateFromSnapshot(null, snapshot), retry };
}
