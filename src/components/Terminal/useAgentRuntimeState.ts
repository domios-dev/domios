import { useEffect, useRef, useState } from "react";
import { invoke, listen } from "../../lib/bridge";
import {
  beginRuntimeRetry, runtimeRetryFailed, runtimeViewFromEvent, runtimeStateFromSnapshot,
  type AgentRuntimeState, type AgentRuntimeStateEvent,
} from "../../lib/agentRuntimeState";

import type { AgentRuntimeQuestion } from "../../lib/agentRuntimeQuestions";

const EMPTY_QUESTIONS: AgentRuntimeQuestion[] = [];

export function useAgentRuntimeState(tabId: string, nonce: string | null, snapshot?: AgentRuntimeState | null, snapshotQuestions: AgentRuntimeQuestion[] = EMPTY_QUESTIONS) {
  const key = JSON.stringify([tabId, nonce]);
  const [record, setRecord] = useState(() => ({ key, state: runtimeStateFromSnapshot(null, snapshot), questions: snapshotQuestions }));
  const current = useRef(record);
  const revision = useRef(0);
  const generation = useRef(0);
  const snapshotState = snapshot?.state;
  const snapshotEpoch = snapshot?.epoch;
  const snapshotReason = snapshot?.reason;
  const snapshotStartedAt = snapshot?.active_turn_started_at_ms;
  const snapshotDuration = snapshot?.last_turn_duration_ms;
  const pending = useRef(false);
  const publish = (state: AgentRuntimeState | null) => {
    current.current = { key, state, questions: current.current.key === key ? current.current.questions : snapshotQuestions };
    setRecord(current.current);
  };

  useEffect(() => {
    revision.current++;
    const next = snapshotState !== undefined && snapshotEpoch !== undefined
      ? { state: snapshotState, epoch: snapshotEpoch, reason: snapshotReason,
        active_turn_started_at_ms: snapshotStartedAt, last_turn_duration_ms: snapshotDuration } : null;
    const state = runtimeStateFromSnapshot(current.current.key === key ? current.current.state : null, next);
    if (current.current.key === key && current.current.state === state && current.current.questions === snapshotQuestions) return;
    current.current = { key, state, questions: snapshotQuestions };
    setRecord(current.current);
  }, [key, snapshotState, snapshotEpoch, snapshotReason, snapshotStartedAt, snapshotDuration, snapshotQuestions]);

  useEffect(() => {
    generation.current++;
    pending.current = false;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const eventName = tabId.startsWith("mesh:") ? `agent-runtime-state:${tabId}` : "agent-runtime-state";
    void listen<AgentRuntimeStateEvent>(eventName, ({ payload }) => {
      if (disposed) return;
      const next = runtimeViewFromEvent(current.current, { tabId, nonce }, payload);
      if (next === current.current) return;
      revision.current++;
      current.current = { ...next, key };
      setRecord(current.current);
    }).then((stop) => { if (disposed) stop(); else unlisten = stop; }).catch((error: unknown) => {
      if (disposed || !current.current.state) return;
      revision.current++;
      const state = { ...current.current.state, state: "degraded" as const,
        reason: error instanceof Error ? error.message : String(error) };
      current.current = { ...current.current, key, state };
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

  return { questions: record.key === key ? record.questions : snapshotQuestions, state: record.key === key ? record.state : runtimeStateFromSnapshot(null, snapshot), retry };
}
