import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { BackgroundProcessEntry } from "../../lib/backgroundProcesses";
import type { ChildTranscriptAuthority } from "../../lib/childTranscript";

export const LONG_RUNNING_WORK_MS = 15_000;

export interface ComposerBackgroundWorkItem {
  id: string;
  kind: "agent" | "terminal";
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
  cwd?: string;
  osPid?: number | null;
}

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
    const prompt = string(source.agent_description);
    const path = string(source.agent_path);
    const current = authority?.rootThreadId === rootThreadId && source.agent_binding_nonce === authority?.bindingNonce && source.agent_runtime_epoch === authority?.epoch;
    latest.set(key, {
      order,
      item: {
        id: `agent:${threadId}`,
        kind: "agent",
        name: shortName(path, prompt ?? threadId),
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
  const terminalItems = terminals.flatMap((terminal): ComposerBackgroundWorkItem[] => {
    const sourceMessage = messagesById.get(terminal.itemId);
    const startedAtMs = timestampMs(sourceMessage?.timestamp);
    if (startedAtMs === undefined || nowMs - startedAtMs < minDurationMs) return [];
    return [{
      id: `terminal:${terminal.threadId}:${terminal.processId}`,
      kind: "terminal",
      name: shortName(terminal.command.split(/\s+/)[0], "Terminal"),
      threadId: terminal.threadId,
      startedAtMs,
      elapsed: backgroundWorkElapsed(nowMs - startedAtMs),
      command: terminal.command,
      outputTail: messageOutput(sourceMessage),
      processId: terminal.processId,
      cwd: terminal.cwd,
      osPid: terminal.osPid,
    }];
  });
  return [...terminalItems, ...(subagentsVisible && rootThreadId ? agentWork(messages, rootThreadId, nowMs, minDurationMs, options.authority) : [])]
    .sort((a, b) => a.startedAtMs - b.startedAtMs);
}

export function composerBackgroundWorkBubble(items: readonly ComposerBackgroundWorkItem[]): ComposerBackgroundWorkBubble | null {
  return items.length ? { count: items.length, items: [...items] } : null;
}
