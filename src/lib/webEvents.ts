/**
 * Web-mode event delivery for the `listen()` bridge.
 *
 * Primary: the daemon's SSE stream (`GET /api/events/stream`), consumed
 * via fetch + ReadableStream because EventSource can't send the
 * Authorization header. Reconnects with backoff, and `resetWebEvents()`
 * tears the connection down so a peer switch reattaches to the new host.
 *
 * Belt-and-braces: a coarse 5s tick fires the well-known refresh events
 * (most listeners treat events purely as "go refetch now" signals), so
 * the UI stays live even for emits the daemon doesn't mirror to SSE yet.
 */
import { apiHost, currentToken, IS_WEB } from "./webApi";

type Handler = (e: { payload: unknown }) => void;

const handlers = new Map<string, Set<Handler>>();
let streamAbort: AbortController | null = null;
let tickTimer: ReturnType<typeof setInterval> | null = null;
let started = false;

const POLL_EVENTS = ["mux-state-changed", "mesh-state-changed", "pane-status-changed"];
const POLL_INTERVAL_MS = 5_000;

function dispatch(event: string, payload: unknown): void {
  const set = handlers.get(event);
  if (!set) return;
  for (const h of set) {
    try {
      h({ payload });
    } catch (e) {
      console.warn(`[web-events] handler for ${event} threw:`, e);
    }
  }
}

async function runStream(): Promise<void> {
  let backoffMs = 1_000;
  for (;;) {
    const abort = new AbortController();
    streamAbort = abort;
    try {
      const token = currentToken();
      const res = await fetch(`${apiHost()}/api/events/stream`, {
        headers: token ? { Authorization: `Bearer ${token}` } : {},
        signal: abort.signal,
      });
      if (!res.ok || !res.body) throw new Error(`HTTP ${res.status}`);
      backoffMs = 1_000;
      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buf = "";
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        buf += decoder.decode(value, { stream: true });
        let idx;
        while ((idx = buf.indexOf("\n\n")) >= 0) {
          const frame = buf.slice(0, idx);
          buf = buf.slice(idx + 2);
          const dataLine = frame
            .split("\n")
            .find((l) => l.startsWith("data:"));
          if (!dataLine) continue;
          try {
            const msg = JSON.parse(dataLine.slice(5).trim()) as {
              event_type: string;
              data: unknown;
            };
            dispatch(msg.event_type, msg.data);
          } catch {
            /* malformed frame — skip */
          }
        }
      }
    } catch {
      /* connection lost / aborted — fall through to retry */
    }
    if (abort.signal.aborted && streamAbort !== abort) {
      // Torn down by resetWebEvents (peer switch): the new loop owns it.
      return;
    }
    await new Promise((r) => setTimeout(r, backoffMs));
    backoffMs = Math.min(backoffMs * 2, 15_000);
  }
}

function ensureStarted(): void {
  if (started || !IS_WEB) return;
  started = true;
  void runStream();
  tickTimer = setInterval(() => {
    for (const ev of POLL_EVENTS) dispatch(ev, null);
  }, POLL_INTERVAL_MS);
}

export function webListen(event: string, handler: Handler): () => void {
  ensureStarted();
  let set = handlers.get(event);
  if (!set) {
    set = new Set();
    handlers.set(event, set);
  }
  set.add(handler);
  return () => {
    set!.delete(handler);
  };
}

/** Tear down the SSE connection (peer switch) — it reconnects to the
 * current apiHost automatically. */
export function resetWebEvents(): void {
  if (tickTimer) {
    clearInterval(tickTimer);
    tickTimer = null;
  }
  started = false;
  streamAbort?.abort();
  streamAbort = null;
  ensureStarted();
}
