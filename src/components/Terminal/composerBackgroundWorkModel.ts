import type { NormalizedChatMessage, ToolEvent } from "@codeterm/chat-core";
import { materializeToolEvent, reduceToolEvents, toolCommand } from "@codeterm/chat-core";
import type { BackgroundProcessEntry } from "../../lib/backgroundProcesses";
import { groupBackgroundProcesses, type BackgroundProcessMember } from "../../lib/backgroundProcessGroups";
import type { ChildTranscriptAuthority } from "../../lib/childTranscript";
import type { ProviderBackgroundRoster } from "../../lib/providerSignals";
import { activityLabel } from "../../../packages/plugin-sdk/src/activityLabel";

export interface ComposerBackgroundWorkItem {
  id: string;
  kind: "agent" | "terminal" | "shell" | "unknown";
  name: string;
  threadId: string;
  transcriptThreadId?: string;
  startedAtMs?: number;
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
  processes?: readonly BackgroundProcessMember[];
  helperCount?: number;
  memberProcessIds?: readonly string[];
  memberItemIds?: readonly string[];
  /** Set when the provider gave nothing readable. */
  ordinal?: number;
  /** The provider's own id, kept for the tooltip and the accessible description. */
  rawId?: string;
}

export const SUBAGENT_NAME = "Sub-agent";
export const BACKGROUND_PROCESS_NAME = "Background process";
export const BACKGROUND_TASK_NAME = "Background task";

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

const SHELL_INTERPRETERS = new Set(["sh", "bash", "zsh", "dash"]);
const POWERSHELL_INTERPRETERS = new Set(["pwsh", "powershell"]);
const SCRIPT_INTERPRETERS = new Set(["node", "npx", "bun", "bunx", "deno", "python", "python3", "py", "ruby", "perl", "tsx", "ts-node"]);
const CODE_FLAGS = new Set(["-e", "--eval", "-c", "-p", "--print"]);
const VALUE_FLAGS = new Set(["-o", "-p", "--package", "-executionpolicy", "-windowstyle", "--import", "-r", "--require", "-m"]);

function truncateLabel(value: string, max = 24): string {
  return value.length > max ? `${value.slice(0, max - 1)}…` : value;
}

function executableLeaf(raw: string): string {
  const parts = raw.split(/[\\/]/).filter(Boolean);
  const leaf = parts[parts.length - 1] ?? raw;
  return leaf.replace(/\.(?:exe|cmd|bat)$/i, "");
}

function tokenizeShell(input: string): string[] {
  const tokens: string[] = [];
  let current = "";
  let quote: "'" | '"' | null = null;
  for (let i = 0; i < input.length; i++) {
    const ch = input[i];
    if (quote) {
      if (ch === "\\" && quote === '"' && input[i + 1] === '"') {
        current += '"';
        i++;
      } else if (ch === quote) {
        quote = null;
      } else {
        current += ch;
      }
      continue;
    }
    if (ch === "'" || ch === '"') {
      quote = ch;
    } else if (/\s/.test(ch)) {
      if (current) {
        tokens.push(current);
        current = "";
      }
    } else {
      current += ch;
    }
  }
  if (current) tokens.push(current);
  return tokens;
}

function firstPipelineSegment(input: string): string {
  let quote: "'" | '"' | null = null;
  for (let i = 0; i < input.length; i++) {
    const ch = input[i];
    if (quote) {
      if (ch === "\\" && quote === '"' && input[i + 1] === '"') i++;
      else if (ch === quote) quote = null;
      continue;
    }
    if (ch === "'" || ch === '"') {
      quote = ch;
      continue;
    }
    if (ch === "|" || ch === ";" || (ch === "&" && input[i + 1] === "&")) {
      return input.slice(0, i).trim();
    }
  }
  return input.trim();
}

function unwrapShellWrapper(command: string): string {
  const tokens = tokenizeShell(command.trim());
  if (tokens.length < 3) return command.trim();
  const exe = executableLeaf(tokens[0]).toLowerCase();
  const prev = tokens[tokens.length - 2].toLowerCase();
  const isWrapper = (SHELL_INTERPRETERS.has(exe) && (prev === "-c" || prev === "-lc" || prev === "-cl"))
    || (POWERSHELL_INTERPRETERS.has(exe) && (prev === "-command" || prev === "-c"))
    || (exe === "cmd" && prev === "/c");
  return isWrapper ? tokens[tokens.length - 1].trim() : command.trim();
}

function isRedirectionToken(token: string): boolean {
  return /^[0-9]*[<>]/.test(token) || token === "&";
}

export function backgroundCommandName(command: string): string {
  const trimmed = command.trim();
  if (!trimmed) return "Command unavailable";
  const unwrapped = unwrapShellWrapper(trimmed);
  const segment = firstPipelineSegment(unwrapped);
  const rawTokens = tokenizeShell(segment).filter((token) => !isRedirectionToken(token));
  let index = 0;
  while (index < rawTokens.length && (/^[A-Za-z_][A-Za-z0-9_]*=/.test(rawTokens[index]) || rawTokens[index] === "env" || rawTokens[index] === "nohup")) {
    index++;
  }
  if (index >= rawTokens.length) return activityLabel({ toolKind: "command", toolInput: { command } });

  const head = executableLeaf(rawTokens[index]).toLowerCase();
  const isInterpreter = SHELL_INTERPRETERS.has(head) || POWERSHELL_INTERPRETERS.has(head) || SCRIPT_INTERPRETERS.has(head);
  if (isInterpreter) {
    let probe = index + 1;
    let inlineCode = false;
    while (probe < rawTokens.length && rawTokens[probe].startsWith("-")) {
      const flag = rawTokens[probe].toLowerCase();
      if (SCRIPT_INTERPRETERS.has(head) && CODE_FLAGS.has(flag)) {
        inlineCode = true;
        break;
      }
      probe++;
      if (VALUE_FLAGS.has(flag) && probe < rawTokens.length) probe++;
    }
    if (!inlineCode && probe < rawTokens.length) {
      index = probe;
    }
  }

  const target = executableLeaf(rawTokens[index]);
  if (!target) return activityLabel({ toolKind: "command", toolInput: { command } });
  const next = rawTokens[index + 1];
  const includeArg = Boolean(
    next
    && !next.startsWith("-")
    && !isRedirectionToken(next)
    && !/[\r\n]/.test(next)
    && !/[\\/]/.test(next),
  );
  const label = includeArg ? `${target} ${next}` : target;
  return truncateLabel(label, 24);
}

export function backgroundOwnsTool(tool: { id?: string; sourceSessionId?: string } | undefined, items: readonly ComposerBackgroundWorkItem[]): boolean {
  const toolId = tool?.id;
  const sessionId = tool?.sourceSessionId;
  return toolId !== undefined && items.some(item => item.kind === "terminal"
    && (item.sourceItemId === toolId || item.memberItemIds?.includes(toolId) === true)
    && (!sessionId || sessionId === item.threadId));
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

function agentWork(messages: readonly NormalizedChatMessage[], rootThreadId: string, nowMs: number, authority?: ChildTranscriptAuthority): ComposerBackgroundWorkItem[] {
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
        name: readable ? shortName(undefined, readable) : SUBAGENT_NAME,
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
    .filter((item) => item.status === "started" || item.status === "running" || item.status === "unknown");
}

export function composerBackgroundWorkItems(
  messages: readonly NormalizedChatMessage[],
  terminals: readonly BackgroundProcessEntry[],
  nowMs: number,
  options: { subagentsVisible?: boolean; rootThreadId?: string; authority?: ChildTranscriptAuthority } = {},
): ComposerBackgroundWorkItem[] {
  const { subagentsVisible = false, rootThreadId } = options;
  const messagesById = new Map(messages.map((message) => [message.id, message]));
  const groups = groupBackgroundProcesses(terminals);
  const terminalItems = groups.map((group): ComposerBackgroundWorkItem => {
    const terminal = group.primary;
    const sourceMessage = group.itemIds
      .map((itemId) => messagesById.get(itemId))
      .find((message) => message && (!message.sourceSessionId || message.sourceSessionId === terminal.threadId));
    const startedAtMs = timestampMs(sourceMessage?.timestamp);
    return {
      id: `terminal:${terminal.threadId}:${terminal.processId}`,
      kind: "terminal",
      name: backgroundCommandName(terminal.command),
      threadId: terminal.threadId,
      startedAtMs,
      elapsed: startedAtMs === undefined ? "" : backgroundWorkElapsed(nowMs - startedAtMs),
      command: terminal.command,
      outputTail: messageOutput(sourceMessage),
      processId: terminal.processId,
      sourceItemId: terminal.itemId,
      status: "running",
      cwd: terminal.cwd,
      osPid: terminal.osPid,
      processes: group.members,
      helperCount: group.helperCount,
      memberProcessIds: group.processIds,
      memberItemIds: group.itemIds,
    };
  });
  return [...terminalItems, ...(subagentsVisible && rootThreadId ? agentWork(messages, rootThreadId, nowMs, options.authority) : [])]
    .sort((a, b) => (a.startedAtMs ?? Infinity) - (b.startedAtMs ?? Infinity));
}

export function isRunningWork(item: ComposerBackgroundWorkItem): boolean {
  return item.status === "running" || item.status === "started";
}

/** Only live work is shown; without anything running there is no bubble at all, and stale unknown work rides along only beside live work. */
export function composerBackgroundWorkBubble(items: readonly ComposerBackgroundWorkItem[]): ComposerBackgroundWorkBubble | null {
  const count = items.filter(isRunningWork).length;
  return count ? { count, items: items.filter((item) => isRunningWork(item) || item.status === "unknown") } : null;
}

function numberUnnamedSubagents(items: ComposerBackgroundWorkItem[]): ComposerBackgroundWorkItem[] {
  let ordinal = 0;
  return items.map((item) => item.kind === "agent" && item.name === SUBAGENT_NAME ? { ...item, ordinal: ++ordinal } : item);
}

function unwrapToolEvents(event: ToolEvent): ToolEvent[] {
  if (event.stream?.members.length) return event.stream.members.flatMap(unwrapToolEvents);
  if (event.commandBatch?.length) return event.commandBatch.flatMap(unwrapToolEvents);
  return [event];
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
  const commands = new Map<string, string>();
  for (const item of reduceToolEvents(relevant)) {
    if (item.kind !== "tool") continue;
    for (const event of unwrapToolEvents(item.event)) {
      if (event.kind !== "command" || !event.backgroundTaskId) continue;
      const command = toolCommand(event).command.trim();
      if (command) commands.set(event.backgroundTaskId, command);
    }
  }
  const callsByToolId = new Map<string, NormalizedChatMessage[]>();
  for (const message of relevant) {
    if (message.type !== "tool_call" || message.toolKind !== "command" || !message.toolId) continue;
    const list = callsByToolId.get(message.toolId);
    if (list) list.push(message);
    else callsByToolId.set(message.toolId, [message]);
  }
  for (const result of results) {
    const taskId = result.toolBackgroundTaskId;
    if (!taskId || commands.has(taskId) || !result.toolId) continue;
    const matches = callsByToolId.get(result.toolId);
    if (matches?.length !== 1) continue;
    const event = materializeToolEvent(matches[0]);
    const command = event ? toolCommand(event).command.trim() : "";
    if (command) commands.set(taskId, command);
  }
  return commands;
}

export function withProviderBackgroundWork(items: ComposerBackgroundWorkItem[], roster: ProviderBackgroundRoster | undefined, rootThreadId?: string, messages: readonly NormalizedChatMessage[] = []): ComposerBackgroundWorkItem[] {
  if (!rootThreadId || !roster) return numberUnnamedSubagents(items);
  const { running, unknown = {}, kinds = {} } = roster;
  const shellIds = new Set([...Object.keys(running), ...Object.keys(unknown)].filter(id => kinds[id] === "shell"));
  const commands = shellIds.size ? providerBackgroundCommands(messages, rootThreadId, shellIds) : new Map<string, string>();
  const known = new Set(items.flatMap(item => [
    ...(item.memberProcessIds ?? []),
    item.processId ?? item.rawId ?? item.threadId,
  ]));
  return numberUnnamedSubagents([...items, ...[
    ...Object.entries(running).map(([id, label]) => ({id, label: label.trim(), status: "running"})),
    ...Object.entries(unknown).map(([id, label]) => ({id, label: label.trim(), status: "unknown"})),
  ].filter(({id}) => !known.has(id)).map(({id, label, status}): ComposerBackgroundWorkItem => {
    const kind = kinds[id] === "shell" ? "shell" : kinds[id] === "subagent" ? "agent" : "unknown";
    const command = kind === "shell" ? (commands.get(id) || (label || undefined)) : undefined;
    const name = kind === "shell"
      ? (command ? backgroundCommandName(command) : BACKGROUND_PROCESS_NAME)
      : kind === "agent"
        ? (label || SUBAGENT_NAME)
        : (label || BACKGROUND_TASK_NAME);
    return {
      id: `provider:${rootThreadId}:${id}`, kind, rawId: id,
      name,
      command, threadId: id, elapsed: "", status,
      prompt: kind === "shell" ? undefined : (label || undefined),
    };
  })]);
}
