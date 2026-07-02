/**
 * Web-mode event delivery for the `listen()` bridge.
 *
 * Primary: the daemon's SSE stream (`GET /api/events/stream`), consumed via
 * fetch + ReadableStream because EventSource can't send the Authorization
 * header. Frames carry a D1 replay cursor (`seq`+`boot`); reconnects resume
 * with `?since=<cursor>`, mirrored to sessionStorage so it survives reload.
 * `resetWebEvents()` tears the connection down so a peer switch reattaches.
 *
 * Loss has exactly one signal: `event_type: "overflow"` (boot restart, ring
 * eviction, or broadcast lag — server-unified, D1 §3), which triggers a full
 * resync instead of guessing what was missed.
 */
import { apiHost, currentToken, IS_WEB, relayTarget } from "./webApi";

type Handler = (e: { payload: unknown }) => void;

interface EventFrame {
  seq?: number;
  boot?: string;
  event_type: string;
  data: unknown;
}

interface OverflowData {
  reason: "boot-changed" | "ring-evicted" | "lagged";
  boot: string;
  next_seq: number;
  oldest?: number;
}

const handlers = new Map<string, Set<Handler>>();
let streamAbort: AbortController | null = null;
let watchdogTimer: ReturnType<typeof setInterval> | null = null;
let started = false;
let lastFrameAt = 0;
let forceImmediateRetry = false;

const CURSOR_KEY = "codeterm-web-events-cursor";
const RECEIVE_TIMEOUT_MS = 30_000;
const WATCHDOG_INTERVAL_MS = 5_000;

// D1 §4 events confirmed safe to re-dispatch with a null payload (handler
// refetches regardless); mux state uses its own seam below, rest need per-hook wiring.
export const RESYNC_EVENTS = [
  "mesh-state-changed",
  "plan-changed",
  "delayed-send-changed",
  "service-health-changed",
];

function readCursor(): string | null {
  try {
    return sessionStorage.getItem(CURSOR_KEY);
  } catch {
    return null;
  }
}

let cursor: string | null = readCursor();

function saveCursor(next: string): void {
  cursor = next;
  try {
    sessionStorage.setItem(CURSOR_KEY, next);
  } catch {
    /* storage unavailable (private mode) — memory-only cursor still works */
  }
}

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

// Overflow is the ONLY loss signal (D1 §3): adopt the baseline cursor and
// refetch reachable state; coverage beyond this is tracked as follow-up.
function runFullResync(info: OverflowData): void {
  // `next_seq` is one past the last allocated seq (D1 §1a): storing next_seq-1
  // as "last seen" means a later reconnect replays from next_seq forward.
  saveCursor(`${info.boot}:${info.next_seq - 1}`);
  window.dispatchEvent(new Event("codeterm-refresh-mux-state"));
  for (const ev of RESYNC_EVENTS) dispatch(ev, null);
  if (info.reason === "boot-changed") {
    // PTY-layer seam (task item 2): offsets are boot-scoped and need
    // revalidation after a restart; no consumer yet, wired in later.
    window.dispatchEvent(
      new CustomEvent("codeterm-web-boot-changed", { detail: { boot: info.boot } }),
    );
  }
}

function handleFrame(dataLine: string): void {
  let msg: EventFrame;
  try {
    msg = JSON.parse(dataLine.slice(5).trim()) as EventFrame;
  } catch {
    return; // malformed frame — skip
  }
  if (msg.event_type === "overflow") {
    runFullResync(msg.data as OverflowData);
    return;
  }
  if (msg.boot && typeof msg.seq === "number") {
    saveCursor(`${msg.boot}:${msg.seq}`);
  }
  dispatch(msg.event_type, msg.data);
}

async function runStream(): Promise<void> {
  let backoffMs = 1_000;
  for (;;) {
    const abort = new AbortController();
    streamAbort = abort;
    lastFrameAt = Date.now();
    try {
      const token = currentToken();
      const since = cursor ? `?since=${encodeURIComponent(cursor)}` : "";
      const res = await fetch(`${apiHost()}/api/events/stream${since}`, {
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
        lastFrameAt = Date.now();
        buf += decoder.decode(value, { stream: true });
        let idx;
        while ((idx = buf.indexOf("\n\n")) >= 0) {
          const frame = buf.slice(0, idx);
          buf = buf.slice(idx + 2);
          lastFrameAt = Date.now(); // keep-alive comments count as a live frame too (D1 §6)
          const dataLine = frame.split("\n").find((l) => l.startsWith("data:"));
          if (dataLine) handleFrame(dataLine);
        }
      }
    } catch {
      /* connection lost / aborted / watchdog-forced — fall through to retry */
    }
    if (abort.signal.aborted && streamAbort !== abort) {
      // Torn down by resetWebEvents (peer switch): the new loop owns it.
      return;
    }
    if (forceImmediateRetry) {
      forceImmediateRetry = false;
      backoffMs = 1_000;
      continue;
    }
    // Equal jitter: avoids a thundering-herd reconnect after a shared outage.
    const delay = backoffMs / 2 + Math.random() * (backoffMs / 2);
    await new Promise((r) => setTimeout(r, delay));
    backoffMs = Math.min(backoffMs * 2, 15_000);
  }
}

/** D1 §6: no more blind 5s refetch — a receive-timeout watchdog forces a
 * reconnect (which replays via `?since`) if nothing arrives for ~30s. */
function startWatchdog(): void {
  if (watchdogTimer) return;
  watchdogTimer = setInterval(() => {
    if (relayTarget()) return; // no SSE connection to watch in relay mode
    if (Date.now() - lastFrameAt > RECEIVE_TIMEOUT_MS) {
      forceImmediateRetry = true;
      streamAbort?.abort();
    }
  }, WATCHDOG_INTERVAL_MS);
}

function ensureStarted(): void {
  if (started || !IS_WEB) return;
  started = true;
  // Remote peer target: the mesh relay can't stream yet (D2), so skip the SSE
  // connection; the watchdog is a no-op there too until D2 lands.
  if (!relayTarget()) void runStream();
  startWatchdog();
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
 * current apiHost automatically, replaying from the last cursor. */
export function resetWebEvents(): void {
  if (watchdogTimer) {
    clearInterval(watchdogTimer);
    watchdogTimer = null;
  }
  started = false;
  streamAbort?.abort();
  streamAbort = null;
  ensureStarted();
}
