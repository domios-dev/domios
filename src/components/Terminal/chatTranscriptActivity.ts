import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { activityLabel } from "../../../packages/plugin-sdk/src/activityLabel";
import type { TabActivity } from "../../lib/tabActivity";
import type { ProviderDescriptor, TabBacking } from "../../lib/types";

export interface ChatTranscriptActivityRow {
  msg: Pick<NormalizedChatMessage, "id" | "type" | "toolStatus">;
}

export interface ChatTranscriptActivityProjection {
  supported: boolean;
  liveToolId: string | null;
  showWorkingTail: boolean;
}

export function projectChatTranscriptActivity(
  rows: readonly ChatTranscriptActivityRow[],
  activityKind: TabActivity["kind"],
  provider: Pick<ProviderDescriptor, "transport" | "agent_runtime"> | null | undefined,
  backing: TabBacking | null | undefined,
  hasChatBackend: boolean,
): ChatTranscriptActivityProjection {
  const isPty = backing?.kind === "pty" || (backing == null && provider?.transport === "pty");
  const supported = !isPty && (
    provider?.transport === "chat_backend"
    || provider?.agent_runtime !== undefined
    || hasChatBackend
    || backing?.kind === "chat_backend"
  );
  if (!supported || activityKind !== "working") return { supported, liveToolId: null, showWorkingTail: false };

  for (let index = rows.length - 1; index >= 0; index -= 1) {
    const message = rows[index]?.msg;
    if (message?.type === "tool_call" && message.toolStatus === "running") {
      return { supported, liveToolId: message.id, showWorkingTail: false };
    }
  }

  return { supported, liveToolId: null, showWorkingTail: true };
}

export function chatWorkingRowId(tabId: string): string {
  return `chat-working:${tabId}`;
}

export function chatWorkingRowMessage(tabId: string): NormalizedChatMessage {
  return { id: chatWorkingRowId(tabId), type: "status", content: "Working…" };
}

export function chatLiveOperationLabel(message: NormalizedChatMessage): string {
  const label = activityLabel(message);
  return label === "Working" ? message.toolSummary?.trim() || "Tool activity" : label;
}
