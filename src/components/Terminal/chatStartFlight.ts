import { createConsumerFlight } from "../../lib/consumerFlight";
import { createChatAttachFlight } from "./chatAttachFlight";

export function createChatStartFlight() {
  const consumers = createConsumerFlight<unknown>();
  const tabs = new Map<string, { flight: ReturnType<typeof createChatAttachFlight<unknown>>; owners: number }>();
  let sequence = 0;
  return <T>(tabId: string, key: string, start: (signal: AbortSignal) => Promise<T>, signal?: AbortSignal): Promise<T> => {
    return consumers.run(JSON.stringify([tabId, key]), async sharedSignal => {
      let tab = tabs.get(tabId);
      if (!tab) {
        tab = { flight: createChatAttachFlight<unknown>(), owners: 0 };
        tabs.set(tabId, tab);
      }
      const owned = tab;
      owned.owners++;
      try {
        const result = await owned.flight.run(String(++sequence), () => {
          sharedSignal.throwIfAborted();
          return start(sharedSignal);
        });
        if (result.kind === "superseded") throw new DOMException("Attachment superseded", "AbortError");
        return result.value;
      } finally {
        if (--owned.owners === 0 && tabs.get(tabId) === owned) tabs.delete(tabId);
      }
    }, signal) as Promise<T>;
  };
}

export const runChatStart = createChatStartFlight();

export function canRetryChatStart(state: string, cause: string): boolean {
  return state !== "failed" || ["retry", "authority_changed", "history_settled", "resync:epoch_changed",
    "tab_activated", "window_activated", "resync:connected", "listener_owned"].includes(cause);
}
