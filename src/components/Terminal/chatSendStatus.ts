import { invoke } from "../../lib/bridge";
import { failChatSendRequest, observeDeliveryStatus, reconcileDeliveryStatuses, readChatSends } from "./chatSendStore";

export type ChatSendStatus = {
  send_id: string;
  tab_id: string;
  state: "held" | "written" | "submit_unconfirmed" | "delivered" | "failed";
  error: string | null;
  ledger_id: string;
  sequence: number;
  retained_from: number;
  correlationId?: string;
  pending_reason?: string;
  pad_file?: string;
  system_send?: { content: string; sent_at_ms: number };
};

export async function reconcileTrackedChatMessages(tabId: string): Promise<ChatSendStatus[]> {
  const requested = readChatSends(tabId);
  const statuses = await invoke<ChatSendStatus[]>("chat_send_status", { tabId });
  reconcileDeliveryStatuses(tabId, statuses, requested);
  return statuses;
}

export async function sendTrackedChatMessage(
  tabId: string,
  messageId: string,
  message: string,
): Promise<ChatSendStatus> {
  try {
    const status = await invoke<ChatSendStatus>("chat_send_text", {
      tabId,
      text: message,
      clientToken: messageId,
    });
    observeDeliveryStatus(status);
    return status;
  } catch (error) {
    if (isProvenAdmissionRefusal(error)) failChatSendRequest(tabId, messageId);
    else void reconcileTrackedChatMessages(tabId).catch(() => {});
    console.warn("[chat-send] optimistic send request failed", {
      tabId,
      messageId,
      error: error instanceof Error ? error.message : String(error),
    });
    throw error;
  }
}

export function isProvenAdmissionRefusal(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  if (message.includes("admission_unknown:")) return false;
  return [
    "user correlation token exceeds metadata bound",
    "user send capacity: 32 unsettled admissions",
    "user admission sequence exhausted",
    "user token retention capacity reached",
    "user original exceeds 4 MiB storage bound",
  ].some((refusal) => message.includes(refusal));
}
