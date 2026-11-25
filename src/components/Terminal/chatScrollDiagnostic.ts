import { collectChatTraceDiagnostic } from "./chatGestureTraceRuntime";
import { DEV_BUILD } from "../../lib/devBuild";
import { frontendBootDiagnostic } from "../../lib/frontendBootDiagnostic";

export const CHAT_SCROLL_DIAGNOSTIC_BURST = 8;
export const CHAT_SCROLL_DIAGNOSTIC_REFILL_MS = 2000;

export interface ChatScrollDiagnosticRecord {
  scope: string;
  edge: string;
  at: number;
  state: object;
}

export interface ChatScrollDiagnosticPorts {
  now: () => number;
  scheduleFrame: (callback: () => void) => void;
  emit: (record: ChatScrollDiagnosticRecord) => void;
  burst?: number;
  refillMs?: number;
}

interface PendingDiagnostic {
  scope: string;
  edge: string;
  at: number;
  state: object;
  coalesced: number;
}

export function createChatScrollDiagnosticDispatcher(ports: ChatScrollDiagnosticPorts) {
  const burst = ports.burst ?? CHAT_SCROLL_DIAGNOSTIC_BURST;
  const refillMs = ports.refillMs ?? CHAT_SCROLL_DIAGNOSTIC_REFILL_MS;
  const pending = new Map<string, PendingDiagnostic>();
  let scheduled = false;
  let tokens = burst;
  let lastRefillAt: number | null = null;
  let dropped = 0;

  const flush = () => {
    scheduled = false;
    const now = ports.now();
    if (lastRefillAt != null && refillMs > 0) {
      tokens = Math.min(burst, tokens + Math.max(0, now - lastRefillAt) / refillMs);
    }
    lastRefillAt = now;
    const batch = Array.from(pending.values());
    pending.clear();
    for (const entry of batch) {
      if (tokens < 1) {
        dropped += 1 + entry.coalesced;
        continue;
      }
      tokens -= 1;
      const meta: Record<string, number> = {};
      if (entry.coalesced > 0) meta.coalesced = entry.coalesced;
      if (dropped > 0) {
        meta.dropped = dropped;
        dropped = 0;
      }
      const state = Object.keys(meta).length > 0 ? { ...entry.state, ...meta } : entry.state;
      ports.emit({ scope: entry.scope, edge: entry.edge, at: entry.at, state });
    }
  };

  return (scope: string, edge: string, state: object): void => {
    const key = `${scope}\0${edge}`;
    const existing = pending.get(key);
    if (existing) {
      existing.at = ports.now();
      existing.state = state;
      existing.coalesced += 1;
    } else {
      pending.set(key, { scope, edge, at: ports.now(), state, coalesced: 0 });
    }
    if (!scheduled) {
      scheduled = true;
      ports.scheduleFrame(flush);
    }
  };
}

const defaultDispatcher = createChatScrollDiagnosticDispatcher({
  now: () => performance.now(),
  scheduleFrame: (callback) => {
    if (typeof requestAnimationFrame === "function") requestAnimationFrame(() => callback());
    else queueMicrotask(callback);
  },
  emit: (chat) => frontendBootDiagnostic("chat-scroll", { chat }),
});

export function chatScrollDiagnostic(scope: string, edge: string, state: object): void {
  if (DEV_BUILD && !collectChatTraceDiagnostic(scope, edge, state)) {
    defaultDispatcher(scope, edge, state);
  }
}
