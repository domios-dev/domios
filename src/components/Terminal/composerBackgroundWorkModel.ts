import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { BackgroundProcessEntry } from "../../lib/backgroundProcesses";

export const LONG_RUNNING_WORK_MS = 15_000;
export const VISIBLE_WORK_BADGES = 2;

export interface ComposerBackgroundWorkItem {
  id: string;
  kind: "agent" | "terminal";
  name: string;
  threadId: string;
  startedAtMs: number;
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

export type ComposerWorkBadgeLayout =
  | { kind: "empty" }
  | { kind: "individual"; items: ComposerBackgroundWorkItem[] }
  | { kind: "count"; count: number; items: ComposerBackgroundWorkItem[] };

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

function agentWork(messages: readonly NormalizedChatMessage[], nowMs: number, minDurationMs: number): ComposerBackgroundWorkItem[] {
  const latest = new Map<string, { item: ComposerBackgroundWorkItem; order: number }>();
  messages.forEach((message, order) => {
    if (message.type !== "tool_call" || message.toolKind !== "agent" || message.toolName !== "agent_activity") return;
    const source = record(message.toolInput);
    if (!source) return;
    const threadId = string(source.agent_id);
    const parentThreadId = string(source.agent_parent_thread_id);
    const ordinal = source.agent_ordinal;
    const status = string(source.agent_state);
    const startedAtMs = timestampMs(message.timestamp);
    if (!threadId || !parentThreadId || threadId === parentThreadId || !Number.isSafeInteger(ordinal) || Number(ordinal) < 1 || Number(ordinal) > 64 || !status || startedAtMs === undefined) return;
    const previous = latest.get(threadId);
    const earliest = Math.min(previous?.item.startedAtMs ?? startedAtMs, startedAtMs);
    const prompt = string(source.agent_description);
    const path = string(source.agent_path);
    latest.set(threadId, {
      order,
      item: {
        id: `agent:${threadId}`,
        kind: "agent",
        name: shortName(path, prompt ?? threadId),
        threadId,
        startedAtMs: earliest,
        prompt,
        model: string(source.agent_model),
        effort: string(source.agent_reasoning_effort),
        status,
      },
    });
  });
  return [...latest.values()]
    .sort((a, b) => a.order - b.order)
    .map(({ item }) => item)
    .filter((item) => (item.status === "started" || item.status === "running")
      && nowMs - item.startedAtMs >= minDurationMs);
}

export function composerBackgroundWorkItems(
  messages: readonly NormalizedChatMessage[],
  terminals: readonly BackgroundProcessEntry[],
  nowMs: number,
  minDurationMs = LONG_RUNNING_WORK_MS,
): ComposerBackgroundWorkItem[] {
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
      command: terminal.command,
      outputTail: messageOutput(sourceMessage),
      processId: terminal.processId,
      cwd: terminal.cwd,
      osPid: terminal.osPid,
    }];
  });
  return [...terminalItems, ...agentWork(messages, nowMs, minDurationMs)]
    .sort((a, b) => a.startedAtMs - b.startedAtMs);
}

export function composerWorkBadgeLayout(items: readonly ComposerBackgroundWorkItem[]): ComposerWorkBadgeLayout {
  if (!items.length) return { kind: "empty" };
  if (items.length <= VISIBLE_WORK_BADGES) return { kind: "individual", items: [...items] };
  return { kind: "count", count: items.length, items: [...items] };
}
