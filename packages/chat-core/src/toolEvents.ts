import type { NormalizedChatMessage, ToolEdit, ToolEventKind } from "./types";

export interface ToolEvent {
  id: string;
  toolId?: string;
  phase: "call" | "result";
  status: "pending" | "success" | "error";
  kind: ToolEventKind;
  summary: string;
  detail?: string;
  edits?: ToolEdit[];
  result?: string;
  error?: boolean;
  message: NormalizedChatMessage;
  waitBatch?: {
    members: ToolEvent[];
    elapsedMs?: number;
  };
}

export type TimelineItem =
  | { kind: "message"; message: NormalizedChatMessage }
  | { kind: "tool"; event: ToolEvent };

function isInternalOrchestrationTool(name: string | undefined): boolean {
  return name === "send_message"
    || name === "spawn_agent"
    || name === "followup_task"
    || name === "interrupt_agent"
    || name === "list_agents"
    || name === "wait_agent";
}

function text(value: string | undefined): string {
  return value?.trim() ?? "";
}

const WAIT_TOOL_NAMES = new Set(["wait", "tool_wait", "wait_for", "wait_for_result"]);

function isWaitCall(item: TimelineItem): item is { kind: "tool"; event: ToolEvent } {
  if (item.kind !== "tool") return false;
  const { event } = item;
  const name = event.message.toolName?.trim().toLowerCase();
  return event.phase === "call"
    && event.message.type === "tool_call"
    && !event.waitBatch
    && event.kind === "generic"
    && Boolean(name && WAIT_TOOL_NAMES.has(name));
}

function numericTimestamp(message: NormalizedChatMessage): number | undefined {
  if (typeof message.timestamp !== "string" || !message.timestamp.trim()) return undefined;
  const value = Number(message.timestamp);
  return Number.isFinite(value) ? value : undefined;
}

function waitBatch(members: ToolEvent[]): ToolEvent {
  const first = members[0];
  const start = numericTimestamp(first.message);
  const end = numericTimestamp(members[members.length - 1].message);
  const elapsedMs = start === undefined || end === undefined ? undefined : Math.max(0, end - start);
  return {
    ...first,
    id: `${first.id}:wait-batch`,
    kind: "generic",
    summary: `${members.length} wait ${members.length === 1 ? "event" : "events"}`,
    waitBatch: { members, elapsedMs },
  };
}

/** Groups only adjacent normalized generic wait calls after call/result reconciliation. */
export function coalesceWaitEvents(items: TimelineItem[]): TimelineItem[] {
  const output: TimelineItem[] = [];
  let members: ToolEvent[] = [];
  let sessionId: string | undefined;
  const flush = () => {
    if (members.length) output.push({ kind: "tool", event: waitBatch(members) });
    members = [];
    sessionId = undefined;
  };

  for (const item of items) {
    if (isWaitCall(item) && members.length === 0) {
      members.push(item.event);
      sessionId = item.event.message.sourceSessionId;
    } else if (isWaitCall(item) && sessionId !== undefined && item.event.message.sourceSessionId === sessionId) {
      members.push(item.event);
    } else {
      flush();
      if (isWaitCall(item)) {
        members.push(item.event);
        sessionId = item.event.message.sourceSessionId;
      } else output.push(item);
    }
  }
  flush();
  return output;
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

export function reduceToolEvents(messages: NormalizedChatMessage[]): TimelineItem[] {
  const items: TimelineItem[] = [];
  const calls = new Map<string, { item: number; event: ToolEvent }>();
  const positionalCalls: number[] = [];
  const waiting = new Map<string, ToolEvent[]>();
  const seenResults = new Set<string>();

  for (const message of messages) {
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
    const event = materializeToolEvent(message);
    if (!event) {
      items.push({ kind: "message", message });
      continue;
    }
    if (event.phase === "call") {
      const pending = event.toolId ? waiting.get(event.toolId) : undefined;
      const resolved = pending?.reduce(mergeResult, event) ?? event;
      if (event.toolId) waiting.delete(event.toolId);
      const item = items.length;
      items.push({ kind: "tool", event: resolved });
      if (event.toolId) calls.set(event.toolId, { item, event: resolved });
      else positionalCalls.push(item);
      continue;
    }
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
  return coalesceWaitEvents(items);
}
