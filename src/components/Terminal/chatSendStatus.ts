import { invoke } from "../../lib/bridge";
import { beginChatSendDeletion, chatSendTabHydrationState, endChatSendDeletion, ensureChatSendTabHydrated,
  failChatSendRequest, observeChatSendDismissal, observeDeliveryStatus,
  reconcileChatSendAdmission, reconcileDeliveryStatuses, readChatSends, recordChatSendAdmissionFailure,
  persistLocalChatSendDismissal, type ChatSendDismissal } from "./chatSendStore";
import type { AdmissionEvidence } from "./chatDeliveryModel";

const activeAdmissions = new Set<string>();
const ADMISSION_DEADLINE_MS = 60_000;
const admissionKey = (tabId: string, token: string) => JSON.stringify([tabId, token]);
type ReconciliationRun = { phase: "hydrating" | "reading"; dirty: boolean; promise: Promise<ChatSendStatus[]> };
const reconciliationRuns = new Map<string, ReconciliationRun>();

type WireChatSendStatus = import("../../../gen/ChatSendStatus").ChatSendStatus;
type SystemSendView = Pick<NonNullable<WireChatSendStatus["system_send"]>, "content" | "sent_at_ms">
  & Partial<NonNullable<WireChatSendStatus["system_send"]>>;
export type ChatSendStatus = Omit<WireChatSendStatus, "system_send"> & { system_send?: SystemSendView };

export function reconcileTrackedChatMessages(tabId: string): Promise<ChatSendStatus[]> {
  const existing = reconciliationRuns.get(tabId);
  if (existing) {
    if (existing.phase === "reading") existing.dirty = true;
    return existing.promise;
  }
  const run: ReconciliationRun = { phase: "hydrating", dirty: false, promise: Promise.resolve([]) };
  const task = (async () => {
    let statuses: ChatSendStatus[] = [];
    do {
      run.phase = "hydrating";
      run.dirty = false;
      await ensureChatSendTabHydrated(tabId);
      if (chatSendTabHydrationState(tabId) !== "ready") return [];
      run.phase = "reading";
      statuses = await reconcileReadyChatMessages(tabId);
    } while (run.dirty);
    return statuses;
  })().finally(() => {
    if (reconciliationRuns.get(tabId) === run) reconciliationRuns.delete(tabId);
  });
  run.promise = task;
  reconciliationRuns.set(tabId, run);
  return task;
}

async function reconcileReadyChatMessages(tabId: string): Promise<ChatSendStatus[]> {
  const requested = readChatSends(tabId);
  const [statuses, dismissals] = await Promise.all([
    invoke<ChatSendStatus[]>("chat_send_status", { tabId }),
    invoke<ChatSendDismissal[]>("chat_send_dismissals", { tabId }).catch(() => []),
  ]);
  if (Array.isArray(dismissals)) for (const receipt of dismissals) if (receipt.send?.id) observeChatSendDismissal(tabId, receipt);
  reconcileDeliveryStatuses(tabId, statuses, requested);
  const unresolved = readChatSends(tabId).filter((send) => !send.dismissed && !send.status && !send.admissionSettled && send.correlationId
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
  startupOverrideNonce?: string | null,
): Promise<import("../../../gen/ChatSendResult").ChatSendResult> {
  if (typeof tabId !== "string" || !tabId.trim() || tabId === "null" || tabId === "undefined") {
    throw new Error("Chat tab is not bound");
  }
  const key = admissionKey(tabId, messageId);
  activeAdmissions.add(key);
  let deadline: ReturnType<typeof setTimeout> | undefined;
  try {
    const request = invoke<import("../../../gen/ChatSendResult").ChatSendResult>("chat_send_text", {
      tabId,
      text: message,
      clientToken: messageId,
      ...(startupOverrideNonce ? { startupOverrideNonce } : {}),
    }).then(status => { observeDeliveryStatus(status); return status; });
    const status = await Promise.race([request, new Promise<never>((_resolve, reject) => {
      deadline = setTimeout(() => reject(new Error("admission_unknown: send request is still unconfirmed")), ADMISSION_DEADLINE_MS);
    })]);
    return status;
  } catch (error) {
    activeAdmissions.delete(key);
    if (isProvenAdmissionRefusal(error)) failChatSendRequest(tabId, messageId, error);
    else {
      recordChatSendAdmissionFailure(tabId, messageId, hadDaemonResponse(error), error);
      void reconcileTrackedChatMessages(tabId).catch(() => {});
    }
    console.warn("[chat-send] optimistic send request failed", {
      tabId,
      messageId,
      error: error instanceof Error ? error.message : String(error),
    });
    throw error;
  } finally {
    clearTimeout(deadline);
    activeAdmissions.delete(key);
  }
}

// A deadline or abort ends the request without any daemon reply, exactly like a dropped connection.
function hadDaemonResponse(error: unknown): boolean {
  if (error instanceof TypeError) return false;
  const name = typeof error === "object" && error !== null ? (error as { name?: unknown }).name : undefined;
  return name !== "TimeoutError" && name !== "AbortError";
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
    "startup override ",
    "Startup override ",
  ].some((refusal) => message.includes(refusal));
}

export async function deleteTrackedChatMessage(tabId: string, id: string): Promise<void> {
  const send = beginChatSendDeletion(tabId, id);
  if (!send) return;
  try {
    if (!send.correlationId || send.status?.system_send) {
      await persistLocalChatSendDismissal(tabId, send);
      return;
    }
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
