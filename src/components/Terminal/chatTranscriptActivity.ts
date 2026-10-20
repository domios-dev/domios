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
  if (!supported || activityKind !== "working") return { supported, liveToolId: null };

  for (let index = rows.length - 1; index >= 0; index -= 1) {
    const message = rows[index]?.msg;
    if (message?.type === "tool_call" && message.toolStatus === "running") {
      return { supported, liveToolId: message.id };
    }
  }

  return { supported, liveToolId: null };
}

export function chatLiveOperationLabel(message: NormalizedChatMessage): string {
  const label = activityLabel(message);
  return label === "Working" ? message.toolSummary?.trim() || "Tool activity" : label;
}
