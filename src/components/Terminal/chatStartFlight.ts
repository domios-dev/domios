import { OutcomeUnknown, RetryableMuxError } from "../../lib/streamMux";
import { SseCapacityError } from "../../lib/transportClient";
import { createConsumerFlight } from "../../lib/consumerFlight";

export function createChatStartFlight() {
  const consumers = createConsumerFlight<unknown>();
  const tabs = new Map<string, Promise<unknown>>();
  return <T>(tabId: string, key: string, start: (signal: AbortSignal) => Promise<T>, signal?: AbortSignal): Promise<T> => {
    return consumers.run(JSON.stringify([tabId, key]), sharedSignal => {
      const previous = tabs.get(tabId);
      const attach = () => {
        sharedSignal.throwIfAborted();
        return start(sharedSignal);
      };
      const next = previous ? previous.then(attach, attach) : attach();
      tabs.set(tabId, next);
      return next.finally(() => {
        if (tabs.get(tabId) === next) tabs.delete(tabId);
      });
    }, signal) as Promise<T>;
  };
}

export const runChatStart = createChatStartFlight();

export function canRetryChatStart(state: string, cause: string): boolean {
  return state !== "failed" || ["retry", "authority_changed", "history_settled", "resync:epoch_changed",
    "tab_activated", "window_activated", "resync:reconnected", "listener_owned"].includes(cause);
}


export function isInterruptedChatRead(error: unknown): boolean {
  return error instanceof RetryableMuxError || error instanceof SseCapacityError || error instanceof OutcomeUnknown
    || error instanceof DOMException && error.name === "AbortError";
}
