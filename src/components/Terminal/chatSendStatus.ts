import { invoke } from "../../lib/bridge";
import { failChatSendRequest, observeDeliveryStatus, reconcileDeliveryStatuses, reconcileChatSendAdmission, recordChatSendAdmissionFailure, readChatSends, observeChatSendDismissal, beginChatSendDeletion, endChatSendDeletion, type ChatSendDismissal } from "./chatSendStore";
import type { AdmissionEvidence } from "./chatDeliveryModel";

const activeAdmissions = new Set<string>();
const admissionKey = (tabId: string, token: string) => JSON.stringify([tabId, token]);

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
  const [statuses, dismissals] = await Promise.all([
    invoke<ChatSendStatus[]>("chat_send_status", { tabId }),
    invoke<ChatSendDismissal[]>("chat_send_dismissals", { tabId }).catch(() => []),
  ]);
  if (Array.isArray(dismissals)) for (const receipt of dismissals) if (receipt.send?.id) observeChatSendDismissal(tabId, receipt);
  reconcileDeliveryStatuses(tabId, statuses, requested);
  const unresolved = readChatSends(tabId).filter((send) => !send.dismissed && !send.status && send.correlationId
    && !activeAdmissions.has(admissionKey(tabId, send.correlationId)));
  if (unresolved.length > 0) {
    for (const send of unresolved) reconcileChatSendAdmission(tabId, send.correlationId!, "unknown");
    try {
      const evidence = await invoke<Array<{ correlationId: string; evidence: AdmissionEvidence }>>(
        "chat_send_admissions", { tabId, clientTokens: unresolved.map((send) => send.correlationId) },
      );
      if (Array.isArray(evidence)) for (const item of evidence) {
        if (unresolved.some((send) => send.correlationId === item.correlationId)
          && !activeAdmissions.has(admissionKey(tabId, item.correlationId))) {
          reconcileChatSendAdmission(tabId, item.correlationId, item.evidence);
        }
      }
    } catch { /* Missing proof keeps the bubble uncertain. */ }
  }
  return statuses;
}

export async function sendTrackedChatMessage(
  tabId: string,
  messageId: string,
  message: string,
): Promise<ChatSendStatus> {
  const key = admissionKey(tabId, messageId);
  activeAdmissions.add(key);
  try {
    const status = await invoke<ChatSendStatus>("chat_send_text", {
      tabId,
      text: message,
      clientToken: messageId,
    });
    observeDeliveryStatus(status);
    return status;
  } catch (error) {
    activeAdmissions.delete(key);
    if (isProvenAdmissionRefusal(error)) failChatSendRequest(tabId, messageId);
    else {
      recordChatSendAdmissionFailure(tabId, messageId, !(error instanceof TypeError));
      void reconcileTrackedChatMessages(tabId).catch(() => {});
    }
    console.warn("[chat-send] optimistic send request failed", {
      tabId,
      messageId,
      error: error instanceof Error ? error.message : String(error),
    });
    throw error;
  } finally {
    activeAdmissions.delete(key);
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

export async function deleteTrackedChatMessage(tabId: string, id: string): Promise<void> {
  const send = beginChatSendDeletion(tabId, id);
  if (!send) return;
  try {
    const receipt = await invoke<ChatSendDismissal>("chat_send_delete", {
      tabId, dismissal: { send_id: send.status?.send_id ?? send.id, send: {
        id: send.id, correlationId: send.correlationId, content: send.content,
        sentAtMs: send.sentAtMs, echo: send.echo ?? null,
      } },
    });
    observeChatSendDismissal(tabId, receipt);
  } finally {
    endChatSendDeletion(tabId, id);
  }
}
