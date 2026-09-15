import { agentActivityOf, buildAgentTree, isAgentActivityMessage, type AgentActivityTree, type AgentNode } from "./agentActivity";
import { wellFormedFileDiffs } from "./fileDiffs";
import { assignedCommand, toolCommand, unwrapShellLauncher } from "./shellComposition";
import { normalizeToolRichResult } from "./toolRichResult";
import type { AgentActivity, FileDiff, NormalizedChatMessage, ToolEdit, ToolEventKind, ToolRichResult } from "./types";

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
  status: "pending" | "success" | "error" | "interrupted" | "unresolved";
  kind: ToolEventKind;
  summary: string;
  description?: string;
  title?: string;
  flowId?: string;
  detail?: string;
  edits?: ToolEdit[];
  fileDiffs?: FileDiff[];
  richResult?: ToolRichResult;
  result?: string;
  error?: boolean;
  callSourceId?: string;
  pairing?: "exact" | "unavailable";
  exitCode?: number;
  durationMs?: number;
  interrupted?: boolean;
  backgroundTaskId?: string;
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

function isContextCompaction(message: NormalizedChatMessage): boolean {
  return message.timelineMarker?.kind === "context_compaction";
}

function appendMessage(items: TimelineItem[], message: NormalizedChatMessage): void {
  const previous = items[items.length - 1];
  if (
    isContextCompaction(message) &&
    previous?.kind === "message" &&
    isContextCompaction(previous.message)
  ) {
    return;
  }
  items.push({ kind: "message", message });
}

function firstNonEmpty(...values: (string | undefined)[]): string | undefined {
  return values.find((value) => Boolean(value?.trim()));
}

function hasNonzeroExit(message: NormalizedChatMessage): boolean {
  return message.toolExitCode !== undefined && message.toolExitCode !== 0;
}

function eventStatus(message: NormalizedChatMessage, phase: ToolEvent["phase"]): ToolEvent["status"] {
  if (message.toolError === true || hasNonzeroExit(message)) return "error";
  if (message.toolInterrupted === true) return "interrupted";
  if (phase === "call") {
    return message.toolError === false || message.toolResult ? "success" : "pending";
  }
  return "success";
}

export function materializeToolEvent(message: NormalizedChatMessage): ToolEvent | null {
  if (message.type !== "tool_call" && message.type !== "tool_result") return null;
  const phase = message.type === "tool_call" ? "call" : "result";
  const summary = text(message.toolSummary);
  const embeddedResult = phase === "call" ? firstNonEmpty(message.toolResult) : undefined;
  const fileDiffs = wellFormedFileDiffs(message.fileDiffs);
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
    ...(fileDiffs ? { fileDiffs } : {}),
    richResult: normalizeToolRichResult(message.toolRichResult),
    result: phase === "result"
      ? firstNonEmpty(message.toolDetail, message.toolResult, message.content)
      : embeddedResult,
    error: message.toolError === true || hasNonzeroExit(message) ? true : message.toolError,
    callSourceId: text(message.toolCallSourceId) || undefined,
    pairing: message.toolPairing,
    exitCode: message.toolExitCode,
    durationMs: message.toolDurationMs,
    interrupted: message.toolInterrupted,
    backgroundTaskId: text(message.toolBackgroundTaskId) || undefined,
    status: eventStatus(message, phase),
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
  const fileDiffs = result.fileDiffs ?? call.fileDiffs;
  const richResult = result.richResult ?? call.richResult;
  const message = {
    ...call.message,
    toolResult: mergeText(call.message.toolResult, resultText),
    toolError: result.error === true ? true : call.message.toolError,
    toolCallSourceId: call.message.toolCallSourceId ?? result.message.toolCallSourceId,
    toolPairing: result.message.toolPairing ?? call.message.toolPairing,
    toolExitCode: result.message.toolExitCode ?? call.message.toolExitCode,
    toolDurationMs: result.message.toolDurationMs ?? call.message.toolDurationMs,
    toolInterrupted: result.message.toolInterrupted ?? call.message.toolInterrupted,
    toolBackgroundTaskId: result.message.toolBackgroundTaskId ?? call.message.toolBackgroundTaskId,
    ...(fileDiffs ? { fileDiffs } : {}),
    ...(richResult ? { toolRichResult: richResult } : {}),
  };
  const status = eventStatus(message, "call");
  return {
    ...call,
    ...(fileDiffs ? { fileDiffs } : {}),
    ...(richResult ? { richResult } : {}),
    result: mergeText(call.result, resultText),
    error: status === "error" ? true : call.error,
    status,
    callSourceId: text(message.toolCallSourceId) || undefined,
    pairing: message.toolPairing,
    exitCode: message.toolExitCode,
    durationMs: message.toolDurationMs,
    interrupted: message.toolInterrupted,
    backgroundTaskId: text(message.toolBackgroundTaskId) || undefined,
    message,
  };
}

/** The label a stream may roll to: the source's description, else the command an assignment runs, else the call-derived title. */
export function toolStreamLabel(event: ToolEvent): string | undefined {
  if (event.description) return event.description;
  if (event.kind === "command") {
    const { command, argv } = toolCommand(event);
    const assigned = assignedCommand(command, argv);
    if (assigned) return assigned.label;
  }
  return event.title;
}

function commandFingerprint(value: string): string {
  return value.replace(/\s+/g, " ").trim();
}

/** The command an event ran with any launcher peeled off, whitespace-normalized for comparison. */
function executedCommand(event: ToolEvent): string {
  const { command, argv } = toolCommand(event);
  return commandFingerprint(unwrapShellLauncher(command, argv).command);
}

function flowKey(sessionId: string | undefined, flowId: string): string {
  return `${sessionId ?? ""}:${flowId}`;
}

function streamEvent(members: ToolEvent[], entries: StreamEntry[], active: boolean, open: boolean): ToolEvent {
  const first = members[0];
  const latest = members[members.length - 1];
  const failed = members.some((member) => member.status === "error");
  const head: ToolEvent = {
    ...first,
    id: `${first.id}:stream`,
    // A flow the source has not closed is still running, even when every recorded call settled.
    status: open || members.some((member) => member.status === "pending")
      ? "pending"
      : failed ? "error" : members.some((member) => member.status === "unresolved") ? "unresolved"
        : members.some((member) => member.status === "interrupted") ? "interrupted" : "success",
    summary: toolStreamLabel(latest) ?? latest.summary,
    detail: undefined,
    edits: undefined,
    richResult: undefined,
    result: undefined,
    error: failed || undefined,
    stream: { flowId: first.flowId as string, members, entries, active },
  };
  // Diffs stay with the member that produced them; a stream head shows none of its own.
  delete head.fileDiffs;
  return head;
}

/** A sub-agent's own actions as the same stream the main chat renders; null when none were recorded. */
export function agentActionStream(agent: AgentNode, parent: NormalizedChatMessage): ToolEvent | null {
  const actions = agent.actions ?? [];
  if (actions.length === 0) return null;
  const flowId = `agent:${agent.key}`;
  const live = agent.state === "started" || agent.state === "running";
  const members = actions.flatMap((action, index): ToolEvent[] => {
    const event = materializeToolEvent({
      id: `${parent.id}:${flowId}:${action.id ?? index}`,
      type: "tool_call",
      content: action.summary,
      provider: parent.provider,
      sourceSessionId: parent.sourceSessionId,
      toolId: action.id,
      toolKind: action.kind,
      toolSummary: action.summary,
      toolDetail: action.detail,
      toolDescription: action.description,
      toolFlowId: flowId,
      ...(action.status === "pending" ? {} : { toolError: action.status === "error" }),
    });
    if (!event) return [];
    // A finished agent will never report the missing result.
    return [!live && event.status === "pending" ? { ...event, status: "unresolved" } : event];
  });
  if (members.length === 0) return null;
  return streamEvent(members, members.map((event): StreamEntry => ({ kind: "action", event })), live, live);
}

function agentActivityEvent(message: NormalizedChatMessage, records: AgentActivity[]): ToolEvent {
  return { id: `${message.id}:agents`, phase: "call", status: "success", kind: "task", summary: "Agents", message, agents: buildAgentTree(records) };
}

function callKey(event: ToolEvent): string {
  return text(event.toolId) || event.id;
}

/** Folds calls of one proven flow into a stream, one stream per contiguous run of tool activity. */
function groupToolStreams(
  items: TimelineItem[],
  openFlows: ReadonlySet<string>,
  boundaryFlows: ReadonlySet<string>,
  thoughtKeys: ReadonlyMap<number, string>,
): TimelineItem[] {
  const groups = new Map<string, { flow: string; run: number; indexes: number[] }>();
  // Any visible non-tool row (text, inbound, marker) closes the run; thoughts sit inside streams.
  let run = 0;
  items.forEach((item, index) => {
    if (item.kind === "message") {
      if (item.message.type !== "thinking") run++;
      return;
    }
    if (item.event.phase !== "call" || !item.event.flowId || item.event.stream) return;
    const flow = flowKey(item.event.message.sourceSessionId, item.event.flowId);
    const key = `${run}#${flow}`;
    const group = groups.get(key) ?? { flow, run, indexes: [] };
    group.indexes.push(index);
    groups.set(key, group);
  });
  const lastRun = run;
  const thoughtsByKey = new Map<string, number[]>();
  for (const [index, key] of thoughtKeys) thoughtsByKey.set(key, [...(thoughtsByKey.get(key) ?? []), index]);
  const heads = new Map<number, ToolEvent>();
  const absorbed = new Set<number>();
  for (const { flow: key, run: groupRun, indexes } of groups.values()) {
    const eventAt = (index: number) => (items[index] as { kind: "tool"; event: ToolEvent }).event;
    const firstByCall = new Map<string, number>();
    const latestByCall = new Map<string, ToolEvent>();
    const actionIndexes = indexes.filter((index) => {
      const event = eventAt(index);
      if (event.message.toolEcho) return false;
      const call = callKey(event);
      latestByCall.set(call, event);
      if (firstByCall.has(call)) return false;
      firstByCall.set(call, index);
      return true;
    });
    if (actionIndexes.length < 2) continue;
    const members = actionIndexes.map((index) => latestByCall.get(callKey(eventAt(index))) as ToolEvent);
    // Only a boundary-opened flow proves its source emits no descriptions; call-stamped flows need them.
    const labeled = boundaryFlows.has(key) ? toolStreamLabel : (member: ToolEvent) => member.description;
    if (!members.every((member) => labeled(member))) continue;
    const head = actionIndexes[0];
    const last = actionIndexes[actionIndexes.length - 1];
    const thoughts = (thoughtsByKey.get(key) ?? []).filter((index) => index > head && index < last);
    const memberAt = new Map(actionIndexes.map((index, position) => [index, members[position]]));
    const entries = [...actionIndexes, ...thoughts].sort((a, b) => a - b).map((index): StreamEntry => {
      const item = items[index];
      return item.kind === "tool" ? { kind: "action", event: memberAt.get(index) ?? item.event } : { kind: "thought", id: item.message.id, text: item.message.content };
    });
    // A run closed by a later row no longer inherits its turn's open state; only its own calls can still run.
    const open = groupRun === lastRun && openFlows.has(key);
    const active = open || members.some((member) => member.status === "pending");
    heads.set(head, streamEvent(members, entries, active, open));
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
  const sourceCalls = new Map<string, Array<{ item: number; event: ToolEvent }>>();
  const positionalCalls: number[] = [];
  const waiting = new Map<string, ToolEvent[]>();
  const waitingBySource = new Map<string, ToolEvent[]>();
  const seenResults = new Set<string>();
  const openFlowBySession = new Map<string, string>();
  const boundaryFlows = new Set<string>();
  const supersessions = new Map<string, number>();
  const callEpochs = new Map<number, { session: string; epoch: number }>();
  const thoughtKeys = new Map<number, string>();
  const agentGroups = new Map<string, { item: number; message: NormalizedChatMessage; records: AgentActivity[] }>();
  const supersede = (session: string) => supersessions.set(session, (supersessions.get(session) ?? 0) + 1);
  const sourceKey = (session: string | undefined, source: string | undefined): string | undefined => {
    const value = text(source);
    return value ? `${session ?? ""}:${value}` : undefined;
  };
  const resultMatchesCall = (call: ToolEvent, result: ToolEvent): boolean => {
    if (result.pairing === "exact") {
      if (!result.callSourceId || result.callSourceId !== call.callSourceId) return false;
      return !result.toolId || result.toolId === call.toolId;
    }
    if (result.pairing === "unavailable") return Boolean(result.toolId && result.toolId === call.toolId);
    return !result.toolId || result.toolId === call.toolId;
  };
  const replaceSourceCall = (source: string | undefined, item: number, event: ToolEvent) => {
    if (!source) return;
    const entries = sourceCalls.get(source);
    if (!entries) return;
    const index = entries.findIndex((entry) => entry.item === item);
    if (index >= 0) entries[index] = { item, event };
  };
  const echoHosts = new Set<number>();
  // The nearest earlier call of the same session and turn whose command the echo re-reports.
  const echoHost = (echo: ToolEvent): { item: number; event: ToolEvent; exact: boolean } | undefined => {
    const echoed = executedCommand(echo);
    if (!echoed) return undefined;
    const session = echo.message.sourceSessionId ?? "";
    const epoch = supersessions.get(session) ?? 0;
    for (let index = items.length - 1; index >= 0; index--) {
      const at = callEpochs.get(index);
      if (!at || at.session !== session) continue;
      if (at.epoch !== epoch) return undefined;
      const item = items[index];
      if (item.kind !== "tool" || echoHosts.has(index)) continue;
      const candidate = item.event;
      if (candidate.kind !== "command" || candidate.message.toolEcho) continue;
      if (executedCommand(candidate) === echoed) return { item: index, event: candidate, exact: true };
      if (toolCommand(candidate).command.split(/\n\s*\n/).some((block) => commandFingerprint(block) === echoed)) {
        return { item: index, event: candidate, exact: false };
      }
    }
    return undefined;
  };

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
          message.toolExitCode !== undefined ||
          message.toolDurationMs !== undefined ||
          message.toolInterrupted !== undefined ||
          message.toolBackgroundTaskId?.trim() ||
          message.toolEdits?.length ||
          message.toolInput,
      );
      if (!hasContent) continue;
    }
    const materialized = materializeToolEvent(message);
    if (!materialized) {
      appendMessage(items, message);
      continue;
    }
    if (materialized.phase === "call") {
      const flowId = materialized.flowId ?? openFlowBySession.get(message.sourceSessionId ?? "");
      const event = flowId ? { ...materialized, flowId } : materialized;
      const source = sourceKey(message.sourceSessionId, event.callSourceId);
      const id = text(event.toolId);
      const pendingById = id ? waiting.get(id) ?? [] : [];
      const pendingBySource = source ? waitingBySource.get(source) ?? [] : [];
      const existingSourceCalls = source ? sourceCalls.get(source) ?? [] : [];
      const pending = [...new Map([...pendingById, ...pendingBySource].map((result) => [result.id, result])).values()]
        .filter((result) => result.toolId || result.pairing !== "exact" || existingSourceCalls.length === 0)
        .filter((result) => resultMatchesCall(event, result));
      const resolved = pending.reduce(mergeResult, event);
      if (id) {
        const remaining = pendingById.filter((result) => !pending.includes(result));
        if (remaining.length > 0) waiting.set(id, remaining);
        else waiting.delete(id);
      }
      if (source) {
        const remaining = pendingBySource.filter((result) => !pending.includes(result));
        if (remaining.length > 0) waitingBySource.set(source, remaining);
        else waitingBySource.delete(source);
      }
      const host = resolved.message.toolEcho && resolved.kind === "command" ? echoHost(resolved) : undefined;
      if (host) {
        // Only an exact re-report carries this call's outcome; an echo of one command in a batch is just dropped.
        const merged = host.exact ? mergeResult(host.event, { ...resolved, summary: "" }) : host.event;
        items[host.item] = { kind: "tool", event: merged };
        const hostId = text(merged.toolId);
        if (hostId) calls.set(hostId, { item: host.item, event: merged });
        replaceSourceCall(sourceKey(message.sourceSessionId, merged.callSourceId), host.item, merged);
        if (host.exact) {
          echoHosts.add(host.item);
          if (id) calls.set(id, { item: host.item, event: merged });
        }
        continue;
      }
      const item = items.length;
      items.push({ kind: "tool", event: resolved });
      const session = message.sourceSessionId ?? "";
      callEpochs.set(item, { session, epoch: supersessions.get(session) ?? 0 });
      if (id) calls.set(id, { item, event: resolved });
      if (source) {
        const entries = sourceCalls.get(source) ?? [];
        entries.push({ item, event: resolved });
        sourceCalls.set(source, entries);
      } else if (!event.callSourceId) {
        positionalCalls.push(item);
      }
      continue;
    }
    const event = materialized;
    const resultKey = event.toolId ? `${event.toolId}:${event.id}` : event.id;
    if (seenResults.has(resultKey)) continue;
    seenResults.add(resultKey);
    let call: { item: number; event: ToolEvent } | undefined;
    const source = sourceKey(message.sourceSessionId, event.callSourceId);
    if (event.pairing === "exact") {
      const candidates = source ? sourceCalls.get(source) ?? [] : [];
      const matching = candidates.filter((candidate) => resultMatchesCall(candidate.event, event));
      call = matching.length === 1 ? matching[0] : undefined;
    } else if (event.toolId) {
      call = calls.get(text(event.toolId) ?? "");
    }
    if (!event.toolId && event.pairing !== "exact" && event.pairing !== "unavailable") {
      const item = positionalCalls.pop();
      call = item === undefined ? undefined : {
        item,
        event: (items[item] as { kind: "tool"; event: ToolEvent }).event,
      };
    }
    if (call) {
      const current = items[call.item];
      const merged = mergeResult(current.kind === "tool" ? current.event : call.event, event);
      items[call.item] = { kind: "tool", event: merged };
      const id = text(call.event.toolId);
      if (id) calls.set(id, { item: call.item, event: merged });
      replaceSourceCall(sourceKey(message.sourceSessionId, merged.callSourceId), call.item, merged);
    } else if (source && event.pairing === "exact") {
      const pending = waitingBySource.get(source) ?? [];
      pending.push(event);
      waitingBySource.set(source, pending);
    } else if (event.toolId) {
      const id = text(event.toolId);
      const pending = id ? waiting.get(id) ?? [] : [];
      pending.push(event);
      if (id) waiting.set(id, pending);
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
