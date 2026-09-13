import type { AgentActivity, NormalizedChatMessage, ToolEdit, ToolEventKind } from "./types";
import { agentActivityOf, buildAgentTree, isAgentActivityMessage, type AgentActivityTree } from "./agentActivity";

export type StreamEntry =
  | { kind: "action"; event: ToolEvent }
  | { kind: "thought"; id: string; text: string };

interface ToolStream {
  flowId: string;
  members: ToolEvent[];
  /** Members plus same-session, same-flow thoughts that sit between them, in source order. */
  entries: StreamEntry[];
  active: boolean;
}

export interface ToolEvent {
  id: string;
  toolId?: string;
  phase: "call" | "result";
  /** `unresolved`: the source moved past this call (turn ended, new turn, or user turn) without a result. */
  status: "pending" | "success" | "error" | "unresolved";
  kind: ToolEventKind;
  summary: string;
  description?: string;
  title?: string;
  flowId?: string;
  detail?: string;
  edits?: ToolEdit[];
  result?: string;
  error?: boolean;
  message: NormalizedChatMessage;
  stream?: ToolStream;
  /** Sub-agent activity of one session and flow, folded into one card. */
  agents?: AgentActivityTree;
}

export type TimelineItem =
  | { kind: "message"; message: NormalizedChatMessage }
  | { kind: "tool"; event: ToolEvent };

function isInternalOrchestrationTool(name: string | undefined): boolean {
  return name === "send_message"
    || name === "spawn_agent"
    || name === "followup_task"
    || name === "interrupt_agent"
    || name === "list_agents";
}

function text(value: string | undefined): string {
  return value?.trim() ?? "";
}

function firstNonEmpty(...values: (string | undefined)[]): string | undefined {
  return values.find((value) => Boolean(value?.trim()));
}

export function materializeToolEvent(message: NormalizedChatMessage): ToolEvent | null {
  if (message.type !== "tool_call" && message.type !== "tool_result") return null;
  const phase = message.type === "tool_call" ? "call" : "result";
  const summary = text(message.toolSummary);
  const embeddedResult = phase === "call" ? firstNonEmpty(message.toolResult) : undefined;
  return {
    id: message.id,
    toolId: message.toolId,
    phase,
    kind: message.toolKind ?? "generic",
    summary,
    description: text(message.toolDescription) || undefined,
    title: text(message.toolTitle) || undefined,
    flowId: text(message.toolFlowId) || undefined,
    detail: text(message.toolDetail) || undefined,
    edits: message.toolEdits,
    result: phase === "result"
      ? firstNonEmpty(message.toolDetail, message.toolResult, message.content)
      : embeddedResult,
    error: message.toolError,
    status: phase === "call"
      ? message.toolError === true ? "error" : message.toolError === false || embeddedResult ? "success" : "pending"
      : message.toolError === true ? "error" : "success",
    message,
  };
}

function mergeText(existing: string | undefined, incoming: string | undefined): string | undefined {
  const next = incoming?.trim() ? incoming : undefined;
  if (!next) return existing;
  if (!existing || existing === next || existing.includes(next)) return existing || next;
  if (next.includes(existing)) return next;
  return `${existing}\n${next}`;
}

function mergeResult(call: ToolEvent, result: ToolEvent): ToolEvent {
  const resultText = firstNonEmpty(result.result, result.summary);
  const message = {
    ...call.message,
    toolResult: mergeText(call.message.toolResult, resultText),
    toolError: result.error === true ? true : call.message.toolError,
  };
  return {
    ...call,
    result: mergeText(call.result, resultText),
    error: result.error === true ? true : call.error,
    status: result.error === true || call.status === "error" ? "error" : "success",
    message,
  };
}

/** The label a stream may roll to: the source's description, else the call-derived title. */
export function toolStreamLabel(event: ToolEvent): string | undefined {
  return event.description || event.title;
}

function flowKey(sessionId: string | undefined, flowId: string): string {
  return `${sessionId ?? ""}:${flowId}`;
}

function streamEvent(members: ToolEvent[], entries: StreamEntry[], active: boolean): ToolEvent {
  const first = members[0];
  const latest = members[members.length - 1];
  const failed = members.some((member) => member.status === "error");
  return {
    ...first,
    id: `${first.id}:stream`,
    status: members.some((member) => member.status === "pending")
      ? "pending"
      : failed ? "error" : members.some((member) => member.status === "unresolved") ? "unresolved" : "success",
    summary: toolStreamLabel(latest) ?? latest.summary,
    detail: undefined,
    edits: undefined,
    result: undefined,
    error: failed || undefined,
    stream: { flowId: first.flowId as string, members, entries, active },
  };
}

function agentActivityEvent(message: NormalizedChatMessage, records: AgentActivity[]): ToolEvent {
  return { id: `${message.id}:agents`, phase: "call", status: "success", kind: "task", summary: "Agents", message, agents: buildAgentTree(records) };
}

/** Folds calls that share one proven flow into a stream at the first member's position. */
function groupToolStreams(
  items: TimelineItem[],
  openFlows: ReadonlySet<string>,
  boundaryFlows: ReadonlySet<string>,
  thoughtKeys: ReadonlyMap<number, string>,
): TimelineItem[] {
  const groups = new Map<string, number[]>();
  items.forEach((item, index) => {
    if (item.kind !== "tool" || item.event.phase !== "call" || !item.event.flowId || item.event.stream) return;
    const key = flowKey(item.event.message.sourceSessionId, item.event.flowId);
    const indexes = groups.get(key) ?? [];
    indexes.push(index);
    groups.set(key, indexes);
  });
  const thoughtsByKey = new Map<string, number[]>();
  for (const [index, key] of thoughtKeys) thoughtsByKey.set(key, [...(thoughtsByKey.get(key) ?? []), index]);
  const heads = new Map<number, ToolEvent>();
  const absorbed = new Set<number>();
  for (const [key, indexes] of groups) {
    const eventAt = (index: number) => (items[index] as { kind: "tool"; event: ToolEvent }).event;
    const actionIndexes = indexes.filter((index) => !eventAt(index).message.toolEcho);
    if (actionIndexes.length < 2) continue;
    const members = actionIndexes.map(eventAt);
    // Only a boundary-opened flow proves its source emits no descriptions; call-stamped flows need them.
    const labeled = boundaryFlows.has(key) ? toolStreamLabel : (member: ToolEvent) => member.description;
    if (!members.every((member) => labeled(member))) continue;
    const head = actionIndexes[0];
    const last = actionIndexes[actionIndexes.length - 1];
    const thoughts = (thoughtsByKey.get(key) ?? []).filter((index) => index > head && index < last);
    const entries = [...actionIndexes, ...thoughts].sort((a, b) => a - b).map((index): StreamEntry => {
      const item = items[index];
      return item.kind === "tool" ? { kind: "action", event: item.event } : { kind: "thought", id: item.message.id, text: item.message.content };
    });
    const active = openFlows.has(key) || members.some((member) => member.status === "pending");
    heads.set(head, streamEvent(members, entries, active));
    // An echo re-reports a command a member already shows, so a formed stream drops it.
    for (const index of [...indexes, ...thoughts]) if (index !== head) absorbed.add(index);
  }
  if (heads.size === 0) return items;
  return items.flatMap((item, index): TimelineItem[] => {
    if (absorbed.has(index)) return [];
    const head = heads.get(index);
    return head ? [{ kind: "tool", event: head }] : [item];
  });
}

export function reduceToolEvents(messages: NormalizedChatMessage[]): TimelineItem[] {
  const items: TimelineItem[] = [];
  const calls = new Map<string, { item: number; event: ToolEvent }>();
  const positionalCalls: number[] = [];
  const waiting = new Map<string, ToolEvent[]>();
  const seenResults = new Set<string>();
  const openFlowBySession = new Map<string, string>();
  const boundaryFlows = new Set<string>();
  const supersessions = new Map<string, number>();
  const callEpochs = new Map<number, { session: string; epoch: number }>();
  const thoughtKeys = new Map<number, string>();
  const agentGroups = new Map<string, { item: number; message: NormalizedChatMessage; records: AgentActivity[] }>();
  const supersede = (session: string) => supersessions.set(session, (supersessions.get(session) ?? 0) + 1);

  for (const message of messages) {
    // Only source rows carry seq; an optimistic composer row is no evidence a call was abandoned.
    if (message.type === "user" && message.seq !== undefined) supersede(message.sourceSessionId ?? "");
    if (message.type === "tool_flow") {
      const session = message.sourceSessionId ?? "";
      supersede(session);
      const flowId = text(message.toolFlowId);
      if (message.content === "start" && flowId) {
        openFlowBySession.set(session, flowId);
        boundaryFlows.add(flowKey(message.sourceSessionId, flowId));
      }
      else if (message.content === "end" && (!flowId || openFlowBySession.get(session) === flowId)) openFlowBySession.delete(session);
      continue;
    }
    if (isAgentActivityMessage(message)) {
      const record = agentActivityOf(message);
      if (!record) continue;
      const flowId = text(message.toolFlowId) || openFlowBySession.get(message.sourceSessionId ?? "") || "";
      const key = flowKey(message.sourceSessionId, flowId);
      const group = agentGroups.get(key);
      if (group) group.records.push(record);
      else {
        agentGroups.set(key, { item: items.length, message, records: [record] });
        items.push({ kind: "message", message });
      }
      continue;
    }
    if (message.type === "thinking") {
      const flowId = text(message.toolFlowId) || openFlowBySession.get(message.sourceSessionId ?? "");
      if (flowId && message.content?.trim()) thoughtKeys.set(items.length, flowKey(message.sourceSessionId, flowId));
      items.push({ kind: "message", message });
      continue;
    }
    if (isInternalOrchestrationTool(message.toolName)) continue;
    if (message.type === "tool_call" || message.type === "tool_result") {
      const hasContent = Boolean(
        message.content?.trim() ||
          message.toolSummary?.trim() ||
          message.toolDetail?.trim() ||
          message.toolResult?.trim() ||
          message.toolName?.trim() ||
          message.toolArgs?.trim() ||
          message.toolEdits?.length ||
          message.toolInput,
      );
      if (!hasContent) continue;
    }
    const materialized = materializeToolEvent(message);
    if (!materialized) {
      items.push({ kind: "message", message });
      continue;
    }
    if (materialized.phase === "call") {
      const flowId = materialized.flowId ?? openFlowBySession.get(message.sourceSessionId ?? "");
      const event = flowId ? { ...materialized, flowId } : materialized;
      const pending = event.toolId ? waiting.get(event.toolId) : undefined;
      const resolved = pending?.reduce(mergeResult, event) ?? event;
      if (event.toolId) waiting.delete(event.toolId);
      const item = items.length;
      items.push({ kind: "tool", event: resolved });
      const session = message.sourceSessionId ?? "";
      callEpochs.set(item, { session, epoch: supersessions.get(session) ?? 0 });
      if (event.toolId) calls.set(event.toolId, { item, event: resolved });
      else positionalCalls.push(item);
      continue;
    }
    const event = materialized;
    const resultKey = event.toolId ? `${event.toolId}:${event.id}` : event.id;
    if (seenResults.has(resultKey)) continue;
    seenResults.add(resultKey);
    let call = event.toolId ? calls.get(event.toolId) : undefined;
    if (!event.toolId) {
      const item = positionalCalls.pop();
      call = item === undefined ? undefined : {
        item,
        event: (items[item] as { kind: "tool"; event: ToolEvent }).event,
      };
    }
    if (call) {
      const merged = mergeResult(call.event, event);
      items[call.item] = { kind: "tool", event: merged };
      if (event.toolId) calls.set(event.toolId, { item: call.item, event: merged });
    } else if (event.toolId) {
      const pending = waiting.get(event.toolId) ?? [];
      pending.push(event);
      waiting.set(event.toolId, pending);
    } else {
      items.push({ kind: "tool", event });
    }
  }
  for (const group of agentGroups.values()) {
    items[group.item] = { kind: "tool", event: agentActivityEvent(group.message, group.records) };
  }
  const openFlows = new Set([...openFlowBySession].map(([session, flowId]) => flowKey(session || undefined, flowId)));
  for (const [index, { session, epoch }] of callEpochs) {
    const item = items[index];
    if (item.kind === "tool" && item.event.status === "pending" && (supersessions.get(session) ?? 0) > epoch) {
      items[index] = { kind: "tool", event: { ...item.event, status: "unresolved" } };
    }
  }
  return groupToolStreams(items, openFlows, boundaryFlows, thoughtKeys);
}
