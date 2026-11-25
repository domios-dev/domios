import type { EnrichedChatEntry } from "./chatEnrichment";
import { sendTrackedChatMessage, type ChatSendStatus } from "./chatSendStatus";

export function latestFailedToolCallId(entries: readonly EnrichedChatEntry[]): string | null {
  for (let i = entries.length - 1; i >= 0; i -= 1) {
    const msg = entries[i]?.msg;
    if (msg?.type === "tool_call" && msg.toolError === true) {
      return msg.id;
    }
  }
  return null;
}

export function latestTurnStopNoticeId(entries: readonly EnrichedChatEntry[]): string | null {
  let stopNotice: string | null = null;
  for (let i = entries.length - 1; i >= 0; i -= 1) {
    const msg = entries[i]?.msg;
    if (!msg) continue;
    if (msg.type === "system") {
      if (msg.systemSubtype === "model_unavailable") return null;
      if (msg.systemSubtype === "turn_stop_reason") stopNotice ??= msg.id;
    }
    if (msg.type === "user" || msg.type === "assistant") {
      break;
    }
  }
  return stopNotice;
}

export function submitTurnContinuation(
  tabId: string,
  followSentMessage: () => void,
  addOptimistic: (content: string) => string,
  observeChatSendStatus: (status: ChatSendStatus) => void,
  showComposerError: (message: string) => void,
): void {
  followSentMessage();
  const messageId = addOptimistic("Continue");
  void sendTrackedChatMessage(tabId, messageId, "Continue")
    .then(observeChatSendStatus)
    .catch((error: unknown) => showComposerError(error instanceof Error ? error.message : String(error)));
}
