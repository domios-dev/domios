import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { reduceToolEvents, toolCommand } from "@codeterm/chat-core";
import type { BackgroundProcessEntry } from "../../lib/backgroundProcesses";
import type { ChildTranscriptAuthority } from "../../lib/childTranscript";
import { activityLabel } from "../../../packages/plugin-sdk/src/activityLabel";

export const LONG_RUNNING_WORK_MS = 15_000;

export interface ComposerBackgroundWorkItem {
  id: string;
  kind: "agent" | "terminal" | "shell";
  name: string;
  threadId: string;
  transcriptThreadId?: string;
  startedAtMs: number;
  elapsed: string;
  prompt?: string;
  model?: string;
  effort?: string;
  status?: string;
  command?: string;
  outputTail?: string;
  processId?: string;
  sourceItemId?: string;
  cwd?: string;
  osPid?: number | null;
  /** Set when the provider gave nothing readable: the item shows as "Internal agent" with this number. */
  ordinal?: number;
  /** The provider's own id, kept for the tooltip and the accessible description. */
  rawId?: string;
}

export const INTERNAL_AGENT_NAME = "Internal agent";

export interface ComposerBackgroundWorkBubble {
  count: number;
  items: ComposerBackgroundWorkItem[];
}

function record(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function string(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function timestampMs(value: string | undefined): number | undefined {
  if (!value) return undefined;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : undefined;
}

function shortName(value: string | undefined, fallback: string): string {
  const parts = value?.split(/[\\/]/).filter(Boolean);
  const leaf = parts?.[parts.length - 1];
  const candidate = leaf || value || fallback;
  return candidate.length > 24 ? `${candidate.slice(0, 23)}…` : candidate;
}

export function backgroundCommandName(command: string): string {
  const script = command.match(/([^\s"'\\/]+\.(?:ps1|sh|mjs|cjs|js|py))(?=["'\s]|$)/i)?.[1];
  return shortName(script ?? activityLabel({toolKind:"command",toolInput:{command}}), "Terminal");
}

export function backgroundOwnsTool(tool: { id?: string; sourceSessionId?: string } | undefined, items: readonly ComposerBackgroundWorkItem[]): boolean {
  return tool?.id !== undefined && items.some(item => item.kind === "terminal" && item.sourceItemId === tool.id
    && (!tool.sourceSessionId || tool.sourceSessionId === item.threadId));
}

function messageOutput(message: NormalizedChatMessage | undefined): string | undefined {
  if (!message) return undefined;
  const content = string(message.toolResult) ?? string(message.content);
  return content?.slice(-800);
}

export function backgroundWorkElapsed(elapsedMs: number): string {
  const seconds = Math.floor(Math.max(0, elapsedMs) / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const remainingMinutes = minutes % 60;
  return remainingMinutes ? `${hours}h ${remainingMinutes}m` : `${hours}h`;
}

function agentWork(messages: readonly NormalizedChatMessage[], rootThreadId: string, nowMs: number, minDurationMs: number, authority?: ChildTranscriptAuthority): ComposerBackgroundWorkItem[] {
  const latest = new Map<string, { item: ComposerBackgroundWorkItem; order: number }>();
  messages.forEach((message, order) => {
    if (message.timelineMarker?.kind === "context_compaction" && (!message.sourceSessionId || message.sourceSessionId === rootThreadId)) {
      for (const previous of latest.values()) {
        if (previous.item.status === "running" || previous.item.status === "started") {
          previous.item = { ...previous.item, status: "unknown", elapsed: "" };
        }
      }
      return;
    }
    if (message.type !== "tool_call" || message.toolKind !== "agent" || message.toolName !== "agent_activity") return;
    const source = record(message.toolInput);
    if (!source) return;
    const callId = string(source.agent_call_id);
    const transcriptThreadId = string(source.agent_id);
    const threadId = transcriptThreadId ?? callId;
    const parentThreadId = string(source.agent_parent_thread_id) ?? string(message.sourceSessionId);
    const ordinal = source.agent_ordinal;
    const status = string(source.agent_state);
    const startedAtMs = timestampMs(message.timestamp);
    if (!threadId || !parentThreadId || parentThreadId !== rootThreadId || threadId === parentThreadId
      || (ordinal !== undefined && ordinal !== null && (!Number.isSafeInteger(ordinal) || Number(ordinal) < 1 || Number(ordinal) > 64))
      || !status || startedAtMs === undefined) return;
    const key = callId ? `call:${callId}` : `thread:${threadId}`;
    const previous = latest.get(key);
    const earliest = Math.min(previous?.item.startedAtMs ?? startedAtMs, startedAtMs);
    const prompt = string(source.agent_description) ?? previous?.item.prompt;
    const path = string(source.agent_path);
    const current = authority?.rootThreadId === rootThreadId && source.agent_binding_nonce === authority?.bindingNonce && source.agent_runtime_epoch === authority?.epoch;
    const readable = prompt ?? string(source.agent_type) ?? path;
    latest.set(key, {
      order,
      item: {
        id: `agent:${threadId}`,
        kind: "agent",
        name: readable ? shortName(undefined, readable) : INTERNAL_AGENT_NAME,
        rawId: threadId,
        threadId,
        transcriptThreadId,
        startedAtMs: earliest,
        elapsed: current ? backgroundWorkElapsed(nowMs - earliest) : "",
        prompt,
        model: string(source.agent_model),
        effort: string(source.agent_reasoning_effort),
        status: status === "started" || status === "running" ? current ? status : "unknown" : status,
      },
    });
  });
  return [...latest.values()]
    .sort((a, b) => a.order - b.order)
    .map(({ item }) => item)
    .filter((item) => (item.status === "started" || item.status === "running" || item.status === "unknown")
      && nowMs - item.startedAtMs >= minDurationMs);
}

export function composerBackgroundWorkItems(
  messages: readonly NormalizedChatMessage[],
  terminals: readonly BackgroundProcessEntry[],
  nowMs: number,
  options: { minDurationMs?: number; subagentsVisible?: boolean; rootThreadId?: string; authority?: ChildTranscriptAuthority } = {},
): ComposerBackgroundWorkItem[] {
  const { minDurationMs = LONG_RUNNING_WORK_MS, subagentsVisible = false, rootThreadId } = options;
  const messagesById = new Map(messages.map((message) => [message.id, message]));
  const uniqueTerminals = new Map(terminals.map(terminal => [JSON.stringify([terminal.threadId,terminal.processId]),terminal]));
  const terminalItems = [...uniqueTerminals.values()].flatMap((terminal): ComposerBackgroundWorkItem[] => {
    const sourceMessage = messagesById.get(terminal.itemId);
    const startedAtMs = timestampMs(sourceMessage?.timestamp);
    if (startedAtMs === undefined || nowMs - startedAtMs < minDurationMs) return [];
    return [{
      id: `terminal:${terminal.threadId}:${terminal.processId}`,
      kind: "terminal",
      name: backgroundCommandName(terminal.command),
      threadId: terminal.threadId,
      startedAtMs,
      elapsed: backgroundWorkElapsed(nowMs - startedAtMs),
      command: terminal.command,
      outputTail: messageOutput(sourceMessage),
      processId: terminal.processId,
      sourceItemId: terminal.itemId,
      status: "running",
      cwd: terminal.cwd,
      osPid: terminal.osPid,
    }];
  });
  return [...terminalItems, ...(subagentsVisible && rootThreadId ? agentWork(messages, rootThreadId, nowMs, minDurationMs, options.authority) : [])]
    .sort((a, b) => a.startedAtMs - b.startedAtMs);
}

export function isRunningWork(item: ComposerBackgroundWorkItem): boolean {
  return item.status === "running" || item.status === "started";
}

/** Only live work is shown; without anything running there is no bubble at all, and stale unknown work rides along only beside live work. */
export function composerBackgroundWorkBubble(items: readonly ComposerBackgroundWorkItem[]): ComposerBackgroundWorkBubble | null {
  const count = items.filter(isRunningWork).length;
  return count ? { count, items: items.filter((item) => isRunningWork(item) || item.status === "unknown") } : null;
}

function numberInternalAgents(items: ComposerBackgroundWorkItem[]): ComposerBackgroundWorkItem[] {
  let ordinal = 0;
  return items.map((item) => item.kind === "agent" && item.name === INTERNAL_AGENT_NAME ? { ...item, ordinal: ++ordinal } : item);
}

function providerBackgroundCommands(messages: readonly NormalizedChatMessage[], rootThreadId: string, ids: ReadonlySet<string>): Map<string, string> {
  const results = messages.filter(message => message.sourceSessionId === rootThreadId
    && message.toolBackgroundTaskId && ids.has(message.toolBackgroundTaskId));
  if (!results.length) return new Map();
  const sourceIds = new Set(results.flatMap(message => message.toolCallSourceId ? [message.toolCallSourceId] : []));
  const toolIds = new Set(results.flatMap(message => message.toolId ? [message.toolId] : []));
  const outcomes = new Set(results);
  const relevant = messages.filter(message => message.sourceSessionId === rootThreadId && (outcomes.has(message)
    || message.type === "tool_call" && message.toolKind === "command"
      && (message.toolCallSourceId && sourceIds.has(message.toolCallSourceId) || message.toolId && toolIds.has(message.toolId))));
  return new Map(reduceToolEvents(relevant).flatMap(item => item.kind === "tool" && item.event.kind === "command" && item.event.backgroundTaskId
    ? [[item.event.backgroundTaskId, toolCommand(item.event).command] as const] : []));
}

export function withProviderBackgroundWork(items: ComposerBackgroundWorkItem[], running: Readonly<Record<string, string>>, rootThreadId?: string, unknown: Readonly<Record<string, string>> = {}, messages: readonly NormalizedChatMessage[] = []): ComposerBackgroundWorkItem[] {
  if (!rootThreadId) return numberInternalAgents(items);
  const taskIds = new Set([...Object.keys(running), ...Object.keys(unknown)]);
  const commands = taskIds.size ? providerBackgroundCommands(messages, rootThreadId, taskIds) : new Map<string, string>();
  const known = new Set(items.flatMap(item => [item.threadId, item.processId].filter(Boolean)));
  return numberInternalAgents([...items, ...[
    ...Object.entries(running).map(([id, label]) => ({id, label: label.trim(), status: "running"})),
    ...Object.entries(unknown).map(([id, label]) => ({id, label: label.trim(), status: "unknown"})),
  ].filter(({id}) => !known.has(id)).map(({id, label, status}): ComposerBackgroundWorkItem => ({
    id: `provider:${rootThreadId}:${id}`, kind: commands.has(id) ? "shell" : "agent",
    name: commands.has(id) ? backgroundCommandName(commands.get(id)!) : label || INTERNAL_AGENT_NAME, rawId: id,
    command: commands.get(id),
    threadId: id, startedAtMs: 0, elapsed: "", status, prompt: label || undefined,
  }))]);
}
