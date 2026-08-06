import { invoke } from "../../lib/bridge";

export type ChatSendStatus = {
  send_id: string;
  tab_id: string;
  state: "accepted" | "unknown" | "confirmed" | "failed";
  error: string | null;
  ledger_id: string;
  sequence: number;
  retained_from: number;
  correlationId?: string;
};

export async function sendTrackedChatMessage(
  tabId: string,
  messageId: string,
  message: string,
): Promise<ChatSendStatus> {
  try {
    return await invoke<ChatSendStatus>("chat_send_text", {
      tabId,
      text: message,
      clientToken: messageId,
    });
  } catch (error) {
    console.warn("[chat-send] optimistic send request failed", {
      tabId,
      messageId,
      error: error instanceof Error ? error.message : String(error),
    });
    throw error;
  }
}
