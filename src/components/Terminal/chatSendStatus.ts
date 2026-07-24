import { invoke, listen } from "../../lib/bridge";

export type ChatSendStatus = {
  send_id: string;
  pane_id: string;
  state: "pending" | "sent" | "failed";
  error: string | null;
  ledger_id: string;
  sequence: number;
  retained_from: number;
};

const surfacedFailures = new Map<string, number>();
let currentLedgerId: string | null = null;
let retainedFrom = 0;

function surfaceFailure(status: ChatSendStatus | null, onFailure: (error: string) => void): void {
  if (!status) return;
  if (currentLedgerId !== status.ledger_id) {
    currentLedgerId = status.ledger_id;
    retainedFrom = status.retained_from;
    surfacedFailures.clear();
  } else {
    retainedFrom = Math.max(retainedFrom, status.retained_from);
  }
  for (const [sendId, sequence] of surfacedFailures) {
    if (sequence < retainedFrom) surfacedFailures.delete(sendId);
  }
  if (
    status.sequence < retainedFrom ||
    status.state !== "failed" ||
    !status.error ||
    surfacedFailures.has(status.send_id)
  ) {
    return;
  }
  surfacedFailures.set(status.send_id, status.sequence);
  onFailure(status.error);
}

export function observeChatSendStatus(
  paneId: string,
  onFailure: (error: string) => void,
): () => void {
  let disposed = false;
  let unlisten: (() => void) | null = null;

  const reconcile = () => {
    void invoke<ChatSendStatus[]>("chat_send_status", { paneId })
      .then((statuses) => {
        if (!disposed) {
          for (const status of statuses) surfaceFailure(status, onFailure);
        }
      })
      .catch(() => {});
  };
  void listen<ChatSendStatus>("chat-send-status", (event) => {
    if (!disposed && event.payload.pane_id === paneId) {
      surfaceFailure(event.payload, onFailure);
    }
  }).then(
    (cleanup) => {
      if (disposed) {
        cleanup();
        return;
      }
      unlisten = cleanup;
      reconcile();
    },
    () => {
      if (!disposed) reconcile();
    },
  );

  return () => {
    disposed = true;
    unlisten?.();
  };
}

export function resetChatSendStatusForTest(): void {
  surfacedFailures.clear();
  currentLedgerId = null;
  retainedFrom = 0;
}

export function chatSendFailureTombstoneCountForTest(): number {
  return surfacedFailures.size;
}
