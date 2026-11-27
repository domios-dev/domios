import { toolCommandLabel } from "./shellComposition";
import type { StreamEntry, TimelineItem, ToolEvent } from "./toolEvents";
import type { ToolEventKind } from "./types";

export interface ToolRunIdentity {
  sessionId: string;
  kind: ToolEventKind;
  name: string;
  key: string;
}

export interface ToolRunGroup {
  toolName: string;
  kind: ToolEventKind;
  members: ToolEvent[];
  entries: StreamEntry[];
  runningCount: number;
  failedCount: number;
  latestStatus: ToolEvent["status"];
  errors: ToolEvent[];
}

export interface GroupToolRunsOptions {
  pendingPromptToolIds?: ReadonlySet<string>;
}

const SUMMARY_MAX_CHARS = 140;

function cleanOneLine(value: unknown): string | undefined {
  if (typeof value !== "string") return undefined;
  const compact = value.replace(/[\r\n\t]+/g, " ").replace(/\s+/g, " ").trim();
  if (!compact) return undefined;
  return compact.length > SUMMARY_MAX_CHARS ? `${compact.slice(0, SUMMARY_MAX_CHARS - 1).trimEnd()}…` : compact;
}

function inputRecord(event: ToolEvent): Record<string, unknown> | undefined {
  const input = event.message.toolInput;
  return input && typeof input === "object" && !Array.isArray(input) ? input : undefined;
}

function firstInputString(input: Record<string, unknown> | undefined, keys: readonly string[]): string | undefined {
  if (!input) return undefined;
  for (const key of keys) {
    const cleaned = cleanOneLine(input[key]);
    if (cleaned) return cleaned;
  }
  return undefined;
}

function looksLikeRawJson(value: string): boolean {
  const trimmed = value.trim();
  return (trimmed.startsWith("{") && trimmed.endsWith("}")) || (trimmed.startsWith("[") && trimmed.endsWith("]"));
}

export function hasPendingToolPrompt(event: ToolEvent, pendingPromptToolIds?: ReadonlySet<string>): boolean {
  const { message } = event;
  if (Array.isArray(message.interactive) && message.interactive.length > 0) return true;
  if (pendingPromptToolIds && event.toolId && pendingPromptToolIds.has(event.toolId)) return true;
  if (event.status !== "pending") return false;
  const status = (message.toolStatus as string | undefined)?.trim();
  if (status === "awaiting_approval" || status === "pending_approval" || status === "needs_approval") return true;
  const input = inputRecord(event);
  if (!input) return false;
  return input.awaitingApproval === true
    || input.pendingPermission === true
    || input.approvalRequired === true
    || input.permissionState === "pending"
    || input.approvalStatus === "pending"
    || (input.permissionPrompt !== undefined && input.permissionPrompt !== null);
}

export function normalizedToolIdentity(
  event: ToolEvent,
  pendingPromptToolIds?: ReadonlySet<string>,
): ToolRunIdentity | null {
  if (event.phase !== "call") return null;
  // Edit runs are owned by groupEditRuns; un-diffed edits stay standalone EditCards.
  if (event.kind === "edit") return null;
  if (event.stream || event.commandBatch || event.editRun || event.agents || event.toolRun) return null;
  if (hasPendingToolPrompt(event, pendingPromptToolIds)) return null;
  const name = event.message.toolName?.trim();
  if (!name) return null;
  const sessionId = event.message.sourceSessionId ?? "";
  return {
    sessionId,
    kind: event.kind,
    name,
    key: JSON.stringify([sessionId, event.kind, name]),
  };
}

export function toolCallSummaryLine(event: ToolEvent): string | undefined {
  const toolName = event.message.toolName?.trim() ?? "";
  const input = inputRecord(event);

  const actionSummary = firstInputString(input, ["action_summary", "actionSummary"]);
  if (actionSummary) return actionSummary;

  const description = cleanOneLine(event.description) ?? firstInputString(input, ["description"]);
  if (description) return description;

  const title = cleanOneLine(event.title);
  if (title && title !== toolName) return title;

  const path = firstInputString(input, ["file_path", "filePath", "path"]);
  if (path) return path;

  const firstLocation = event.locations?.[0];
  if (firstLocation?.path.trim()) {
    const loc = firstLocation.line !== undefined ? `${firstLocation.path}:${firstLocation.line}` : firstLocation.path;
    const cleaned = cleanOneLine(loc);
    if (cleaned) return cleaned;
  }

  if (event.kind === "command" || event.kind === "codeterm") {
    const command = cleanOneLine(toolCommandLabel(event))
      ?? firstInputString(input, ["command", "cmd", "commandLine"]);
    if (command && command !== toolName) return command;
  } else {
    const command = firstInputString(input, ["command", "cmd", "commandLine"]);
    if (command) return command;
  }

  const target = firstInputString(input, ["pattern", "query", "url", "title", "prompt"]);
  if (target) return target;

  const summary = cleanOneLine(event.summary);
  if (summary && summary !== toolName && !looksLikeRawJson(summary)) return summary;

  const action = firstInputString(input, ["action"]);
  if (action) return action;

  return undefined;
}

function toolRunEvent(identity: ToolRunIdentity, members: ToolEvent[], entries: StreamEntry[]): ToolEvent {
  const first = members[0];
  const latest = members[members.length - 1];
  const runningCount = members.filter((member) => member.status === "pending").length;
  const errors = members.filter((member) => member.status === "error");
  const failedCount = errors.length;
  const latestStatus = latest.status;
  const status: ToolEvent["status"] = runningCount > 0 ? "pending" : latestStatus;
  const head: ToolEvent = {
    ...first,
    id: `${first.id}:run`,
    status,
    summary: `${identity.name} × ${members.length}`,
    description: undefined,
    title: identity.name,
    detail: undefined,
    edits: undefined,
    richResult: undefined,
    result: undefined,
    exitCode: undefined,
    durationMs: undefined,
    interrupted: status === "interrupted" ? true : undefined,
    error: failedCount > 0 ? true : undefined,
    toolRun: {
      toolName: identity.name,
      kind: identity.kind,
      members,
      entries,
      runningCount,
      failedCount,
      latestStatus,
      errors,
    },
  };
  delete head.fileDiffs;
  delete head.locations;
  return head;
}

/** Folds consecutive calls of one normalized tool identity (with optional same-session thinking between them) into one run. */
export function groupToolRuns(items: TimelineItem[], options?: GroupToolRunsOptions): TimelineItem[] {
  const pendingIds = options?.pendingPromptToolIds;
  const result: TimelineItem[] = [];
  let identity: ToolRunIdentity | null = null;
  let members: ToolEvent[] = [];
  let entries: StreamEntry[] = [];
  let trailingThoughts: TimelineItem[] = [];

  const flush = () => {
    if (identity && members.length >= 2) {
      result.push({ kind: "tool", event: toolRunEvent(identity, members, entries) });
      for (const thought of trailingThoughts) result.push(thought);
    } else if (members.length === 1) {
      result.push({ kind: "tool", event: members[0] });
      for (const thought of trailingThoughts) result.push(thought);
    }
    identity = null;
    members = [];
    entries = [];
    trailingThoughts = [];
  };

  for (const item of items) {
    if (item.kind === "message" && item.message.type === "thinking") {
      if (identity && (item.message.sourceSessionId ?? "") === identity.sessionId) {
        trailingThoughts.push(item);
        continue;
      }
      flush();
      result.push(item);
      continue;
    }

    if (item.kind !== "tool") {
      flush();
      result.push(item);
      continue;
    }

    const nextIdentity = normalizedToolIdentity(item.event, pendingIds);
    if (!nextIdentity) {
      flush();
      result.push(item);
      continue;
    }

    if (identity && identity.key === nextIdentity.key) {
      for (const thought of trailingThoughts) {
        if (thought.kind === "message" && thought.message.content.trim()) {
          entries.push({ kind: "thought", id: thought.message.id, text: thought.message.content });
        }
      }
      trailingThoughts = [];
      members.push(item.event);
      entries.push({ kind: "action", event: item.event });
      continue;
    }

    flush();
    identity = nextIdentity;
    members = [item.event];
    entries = [{ kind: "action", event: item.event }];
  }

  flush();
  return result;
}
