import { useEffect, useState } from "react";
import { useLatestCallback } from "../../hooks/useLatestCallback";
import type { AgentRuntimeState } from "../../lib/agentRuntimeState";
import { tabActivity, type TabActivity } from "../../lib/tabActivity";
import type { Tab } from "../../lib/types";

interface InterruptedTurn {
  session_id: string;
  session_binding_nonce: string;
  epoch: number;
  started_at_ms: number | null;
}

export interface StopReceipt {
  stopped?: boolean;
  interrupted_turn?: InterruptedTurn;
}

export function useStopSettlement(tabId: string, tab: Tab | null | undefined, runtime: AgentRuntimeState | null, activity: TabActivity) {
  const [settled, setSettled] = useState<{ tabId: string; turn: InterruptedTurn } | null>(null);
  const snapshot = tab?.agent_runtime_state;
  const started = runtime?.active_turn_started_at_ms ?? snapshot?.active_turn_started_at_ms;
  const matchesTurn = (turn: InterruptedTurn) => turn.started_at_ms != null
    && turn.session_id === tab?.agent_session_id && turn.session_binding_nonce === tab?.session_binding_nonce
    && turn.epoch === (runtime?.epoch ?? snapshot?.epoch) && turn.started_at_ms === started;
  const canSettle = activity.kind === "working" && settled?.tabId === tabId && matchesTurn(settled.turn);
  useEffect(() => {
    if (settled && !canSettle) setSettled(null);
  }, [settled, canSettle]);
  const confirm = useLatestCallback((requestedTab: string, receipt: StopReceipt | null | undefined) => {
    if (requestedTab === tabId && activity.kind === "working" && receipt?.stopped === true
      && receipt.interrupted_turn && matchesTurn(receipt.interrupted_turn)) {
      setSettled({ tabId, turn: receipt.interrupted_turn });
    }
  }, [tabId]);
  return {
    activity: canSettle ? tabActivity({ ...tab, agent_state: { state: "idle", phase: "interrupted" } }, "Idle") : activity,
    confirm,
  };
}
