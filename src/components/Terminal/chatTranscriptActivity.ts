import type { NormalizedChatMessage, TimelineItem } from "@codeterm/chat-core";
import { activityCommand, activityLabel } from "../../../packages/plugin-sdk/src/activityLabel";
import type { AgentRuntimeState } from "../../lib/agentRuntimeState";
import type { TabActivity } from "../../lib/tabActivity";
import type { ProviderDescriptor, TabBacking } from "../../lib/types";

export interface ChatTranscriptActivityRow {
  msg: Pick<NormalizedChatMessage, "id" | "type" | "toolStatus">;
  item?: TimelineItem;
}

export interface ChatTranscriptActivityProjection {
  supported: boolean;
  liveToolId: string | null;
  label?: string;
}

export function supportsChatTranscriptActivity(
  provider: Pick<ProviderDescriptor, "transport" | "agent_runtime"> | null | undefined,
  backing: TabBacking | null | undefined,
  hasChatBackend: boolean,
): boolean {
  const isPty = backing?.kind === "pty" || (backing == null && provider?.transport === "pty");
  return !isPty && (
    provider?.transport === "chat_backend"
    || provider?.agent_runtime !== undefined
    || hasChatBackend
    || backing?.kind === "chat_backend"
  );
}

export function projectChatTranscriptActivity(
  rows: readonly ChatTranscriptActivityRow[],
  activityKind: TabActivity["kind"],
  provider: Pick<ProviderDescriptor, "transport" | "agent_runtime"> | null | undefined,
  backing: TabBacking | null | undefined,
  hasChatBackend: boolean,
): ChatTranscriptActivityProjection {
  const supported = supportsChatTranscriptActivity(provider, backing, hasChatBackend);
  if (!supported || activityKind !== "working") return { supported, liveToolId: null };

  for (let index = rows.length - 1; index >= 0; index -= 1) {
    const { msg, item } = rows[index];
    if (item?.kind === "tool") {
      const { event } = item;
      const members = event.toolRun?.members ?? event.editRun ?? event.commandBatch ?? event.stream?.members ?? [event];
      for (let memberIndex = members.length - 1; memberIndex >= 0; memberIndex -= 1) {
        const member = members[memberIndex];
        if (member.status !== "pending" || member.message.toolStatus !== "running") continue;
        const label = member.kind === "edit" || member.kind === "read" ? chatLiveOperationLabel(member.message)
          : `Running ${chatLiveOperationFullLabel(member.message)}`;
        return { supported, liveToolId: msg.id, label };
      }
    } else if (msg.type === "tool_call" && msg.toolStatus === "running") {
      return { supported, liveToolId: msg.id };
    }
  }

  return { supported, liveToolId: null };
}

export const CHAT_TOOL_SHOW_DELAY_MS = 750;
const MIN_VISIBLE_MS = 400;

interface RunningTool {
  observedAt: number;
  visibleAt?: number;
  source: NormalizedChatMessage;
  row: NormalizedChatMessage;
}

export function projectVisibleChatTools(
  messages: NormalizedChatMessage[],
  previous: ReadonlyMap<string, RunningTool>,
  now: number,
) {
  const running = new Map<string, RunningTool>();
  let nextChangeAt = Infinity;
  let changed = false;
  const projected: NormalizedChatMessage[] = [];
  for (const message of messages) {
    const prior = previous.get(message.id);
    if (message.type !== "tool_call" || message.toolStatus !== "running") {
      if (prior?.visibleAt !== undefined && now < prior.visibleAt + MIN_VISIBLE_MS) {
        projected.push(prior.row);
        running.set(message.id, prior);
        nextChangeAt = Math.min(nextChangeAt, prior.visibleAt + MIN_VISIBLE_MS);
        changed = true;
      } else {
        projected.push(message);
      }
      continue;
    }
    const observedAt = prior?.observedAt ?? now;
    const showAt = observedAt + CHAT_TOOL_SHOW_DELAY_MS;
    const visibleAt = prior?.visibleAt ?? (now >= showAt ? now : undefined);
    const row = prior?.source === message ? prior.row : {
      ...message,
      toolStartedAtMs: message.toolStartedAtMs ?? observedAt,
    };
    running.set(message.id, { observedAt, visibleAt, source: message, row });
    if (visibleAt === undefined) {
      nextChangeAt = Math.min(nextChangeAt, showAt);
    } else {
      projected.push(row);
    }
    changed = true;
  }
  return { rows: changed ? projected : messages, running, nextChangeAt };
}

/** The tool the composer names as running: none while the runtime that would be running it is down. */
export function composerRunningTool<Message extends Pick<NormalizedChatMessage, "type" | "toolStatus">>(
  messages: readonly Message[],
  runtimeState: AgentRuntimeState["state"] | null | undefined,
): Message | undefined {
  if (runtimeState === "degraded" || runtimeState === "unloaded") return undefined;
  return messages.find((message) => message.type === "tool_call" && message.toolStatus === "running");
}

export function chatLiveOperationLabel(message: NormalizedChatMessage): string {
  const label = activityLabel(message);
  return label === "Working" ? message.toolSummary?.trim() || "Tool activity" : label;
}

export function chatLiveOperationFullLabel(message: NormalizedChatMessage): string {
  const input = message.toolInput;
  const fields = input && typeof input === "object" && !Array.isArray(input) ? input as Record<string, unknown> : {};
  if (message.toolKind === "command" || fields.cmd !== undefined || fields.command !== undefined) {
    const command = activityCommand(message);
    if (command) return command;
  }
  const target = fields.file_path ?? fields.path ?? fields.url ?? fields.query;
  return message.toolDetail?.trim() || (typeof target === "string" ? target : "")
    || message.toolSummary?.trim() || message.toolTitle || message.toolName || "Tool activity";
}
