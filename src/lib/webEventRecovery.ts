import { nextReconnectBackoff, reconnectDelay } from "./webReconnectModel";

export const LOCAL_EVENT_RETRY_MS = 250;
export const LOCAL_EVENT_RETRY_CAP_MS = 750;

export function isLocalEventStream(origin: string, relayed: boolean): boolean {
  if (relayed) return false;
  return ["localhost", "127.0.0.1", "[::1]"].includes(new URL(origin).hostname);
}

export function eventReconnectDelay(backoff: number, random: number, hidden: boolean, local: boolean): number {
  return local
    ? Math.min(LOCAL_EVENT_RETRY_CAP_MS, Math.max(LOCAL_EVENT_RETRY_MS, backoff)) * (0.5 + random * 0.5)
    : reconnectDelay(backoff, random, hidden);
}

export function nextEventReconnectBackoff(backoff: number, hidden: boolean, local: boolean): number {
  return local ? Math.min(LOCAL_EVENT_RETRY_CAP_MS, backoff * 2) : nextReconnectBackoff(backoff, hidden);
}
