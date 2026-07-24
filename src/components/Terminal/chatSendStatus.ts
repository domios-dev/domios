import { invoke, listen } from "../../lib/bridge";

export type ChatSendStatus = {
  send_id: string;
  pane_id: string;
  state: "pending" | "sent" | "failed";
  error: string | null;
};

const surfacedFailures = new Set<string>();

function surfaceFailure(status: ChatSendStatus | null, onFailure: (error: string) => void): void {
  if (status?.state !== "failed" || !status.error || surfacedFailures.has(status.send_id)) return;
  surfacedFailures.add(status.send_id);
  onFailure(status.error);
}

export function observeChatSendStatus(
  paneId: string,
  onFailure: (error: string) => void,
): () => void {
  let disposed = false;
  let unlisten: (() => void) | null = null;

  void listen<ChatSendStatus>("chat-send-status", (event) => {
    if (!disposed && event.payload.pane_id === paneId) {
      surfaceFailure(event.payload, onFailure);
    }
  }).then((cleanup) => {
    if (disposed) {
      cleanup();
      return;
    }
    unlisten = cleanup;
    void invoke<ChatSendStatus[]>("chat_send_status", { paneId })
      .then((statuses) => {
        if (!disposed) {
          for (const status of statuses) surfaceFailure(status, onFailure);
        }
      })
      .catch(() => {});
  });

  return () => {
    disposed = true;
    unlisten?.();
  };
}

export function resetChatSendStatusForTest(): void {
  surfacedFailures.clear();
}
