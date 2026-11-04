import { baseDaemonTransport } from "./daemonBootstrap";

type ClientHealth = {
  event: "connection-lost" | "connection-restored" | "chat-connect-slow" | "preview-slow" | "preview-failed";
  elapsedMs: number;
  tabId?: string;
  listenerMs?: number;
  responseMs?: number;
  adoptionMs?: number;
  paintMs?: number;
};

const pending: ClientHealth[] = [];
let sending = false;
let windowStart = 0;
let count = 0;

export function recordClientHealth(event: ClientHealth): void {
  const now = Date.now();
  if (now - windowStart >= 60_000) { windowStart = now; count = 0; }
  if (++count > 30) {
    if (event.event === "connection-restored") void flushClientHealth();
    return;
  }
  if (pending.length === 32) pending.shift();
  pending.push({ ...event, tabId: event.tabId && /^[a-zA-Z0-9_.-]{1,128}$/.test(event.tabId) ? event.tabId : undefined });
  void flushClientHealth();
}

async function flushClientHealth(): Promise<void> {
  if (sending) return;
  sending = true;
  try {
    while (pending.length) {
      const event = pending[0];
      await baseDaemonTransport().localJson("POST", "/api/frontend/client-health", event, { timeoutMs: 5_000 });
      if (pending[0] === event) pending.shift();
    }
  } catch {
    // Reconnection or the next diagnostic flushes the bounded backlog.
  } finally { sending = false; }
}
