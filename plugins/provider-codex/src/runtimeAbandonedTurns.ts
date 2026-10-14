import type { AgentEvent, ChatMsg } from "@codeterm/plugin-sdk";

export interface ToolLifecycle {
  toolStatus?: "running" | "completed" | "failed" | "interrupted" | "abandoned" | null;
  toolStartedAtMs?: number | null;
  toolCompletedAtMs?: number | null;
}
export type RuntimeItem = ChatMsg & ToolLifecycle;
export interface TurnLifecycleState {
  activeTurnId?: string;
  completed: string[];
  abandoned?: string[];
  items: Record<string, RuntimeItem>;
  seq: number;
}

export function pendingCall(state: TurnLifecycleState): RuntimeItem | undefined {
  return Object.values(state.items).find(row => row.toolFlowId === state.activeTurnId && row.toolStatus === "running");
}

export function runningCallStatus(state: TurnLifecycleState): string | undefined {
  const row = pendingCall(state);
  return row ? Array.from(`Running ${row.toolTitle || row.toolSummary || row.toolName || "tool call"}`).slice(0,160).join("") : undefined;
}

export function abandonOpenTurn(state: TurnLifecycleState, threadId: string, nextTurnId: string): AgentEvent[] {
  const previous = state.activeTurnId;
  if (!previous || previous === nextTurnId || state.completed.includes(previous)) return [];
  state.completed.push(previous);
  (state.abandoned ??= []).push(previous);
  const events: AgentEvent[] = [];
  for (const row of Object.values(state.items)) {
    if (row.toolFlowId !== previous || row.toolStatus !== "running") continue;
    const abandoned: RuntimeItem = {...row,toolStatus:"abandoned",toolInterrupted:true};
    state.items[row.id] = abandoned;
    events.push({kind:"chatItem",threadId,item:abandoned});
  }
  const row: RuntimeItem = {id:`${previous}:abandoned`,seq:++state.seq,type:"system",content:"Turn interrupted by a new turn",systemSubtype:"turn_abandoned",toolFlowId:previous};
  state.items[row.id] = row;
  events.push({kind:"chatItem",threadId,item:row},{kind:"turnState",threadId,turnId:previous,state:"idle",...{detail:"interrupted" as const}});
  return events;
}

export function stampToolLifecycle(row: RuntimeItem, previous: RuntimeItem | undefined, method: string, params: Record<string, unknown>): RuntimeItem {
  if (row.type !== "tool_call") return row;
  if (previous?.toolStatus === "abandoned") return previous;
  const raw = params.item as Record<string,unknown> | undefined;
  const suppliedStart = params.startedAtMs;
  const suppliedEnd = params.completedAtMs;
  const started = previous?.toolStartedAtMs ?? (typeof suppliedStart === "number" ? suppliedStart : undefined);
  if (started === undefined) delete row.toolStartedAtMs; else row.toolStartedAtMs = started;
  row.toolStatus = previous?.toolStatus ?? "running";
  if (method === "item/completed") {
    row.toolStatus = raw?.status === "interrupted" ? "interrupted" : row.toolError ? "failed" : "completed";
    if (typeof suppliedEnd === "number") row.toolCompletedAtMs = suppliedEnd; else delete row.toolCompletedAtMs;
  }
  return row;
}

export function interruptPendingCalls(state: TurnLifecycleState, threadId: string, turnId: string): AgentEvent[] {
  return Object.values(state.items).flatMap(row => {
    if (row.toolFlowId !== turnId || row.toolStatus !== "running") return [];
    const interrupted: RuntimeItem = {...row,toolStatus:"interrupted",toolInterrupted:true};
    state.items[row.id] = interrupted;
    return [{kind:"chatItem" as const,threadId,item:interrupted}];
  });
}
