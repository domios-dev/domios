import { describeError, isAuthoredCause } from "../../lib/describeError";
import { createNoticePublisher, type NoticePorts } from "../../lib/notices";
import { CHAT_ERROR_TOAST_LIFETIME_MS } from "../Notifications/toastLifetime";

export function createChatErrorNotice(tabId: string, ports: NoticePorts & { dismissToast: (id: string) => void }) {
  const id = `chat-error:${tabId}`;
  let lastError: string | null = null;
  let recovered = false;
  return {
    show(message: string) {
      if (message === lastError) return;
      lastError = message; recovered = false;
      const description = describeError(message);
      const microphone = !isAuthoredCause(message) && !message.toLowerCase().includes("needs https") && /microphone|mic api|media device|getusermedia|notallowederror|denied permission|permission denied|not allowed by the user agent/i.test(message);
      createNoticePublisher(ports)({ kind: "toast", tabId, workspaceId: null, id,
        title: description.pending ? description.title : microphone ? "Microphone unavailable" : "Chat error",
        detail: description.pending ? description.cause : microphone ? `${message} Open microphone permissions in Settings if needed.` : message,
        severity: description.pending ? "Info" : "Error", lifetimeMs: CHAT_ERROR_TOAST_LIFETIME_MS });
    },
    recover() { if (recovered) return; recovered = true; lastError = null; ports.dismissToast(id); },
  };
}
