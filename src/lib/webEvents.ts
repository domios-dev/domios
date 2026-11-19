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
import { apiHost, relayTarget } from "./webApi";
import { type StreamEnd, type TransportClient } from "./transportClient";
import { baseDaemonTransport } from "./daemonBootstrap";
import { streamMuxDiagnostics, verifyStreamMuxes, wakeStreamMuxes } from "./streamMux";
import { fireResync } from "./resync";
import { replayCursorForTarget } from "./resyncModel";
import { createSeqGapTracker } from "./eventSeqGaps";
import { RECONNECT_FIRST_MS, shouldResetReconnectBackoff, transitionWebReconnect, type WebReconnectState } from "./webReconnectModel";
import { eventReconnectDelay, isLocalEventStream, LOCAL_EVENT_RETRY_MS, nextEventReconnectBackoff } from "./webEventRecovery";
import { DEV_BUILD } from "./devBuild";
import { TERMINAL_VIEWER_DISCONNECTED_EVENT, TERMINAL_VIEWER_RECONNECTED_EVENT } from "./terminalViewerEvents";
import { signalEventStreamLost, signalEventStreamReady } from "./eventStreamReady";
import { WEB_BOOT_CHANGED_EVENT, WEB_EVENTS_READY_EVENT, type BootChangedDetail } from "./buildDriftReload";

let eventClient: TransportClient | undefined;
export function bindWebEventsTransport(client: TransportClient): void {
  eventClient = client;
  if (started) resetWebEvents();
}
const eventOrigin = () => eventClient?.origin() ?? apiHost();

type Handler = (e: { payload: unknown }) => void;

interface EventFrame {
  seq?: number;
  prev_seq?: number | null;
  boot?: string;
  event_type: string;
  data: unknown;
}

interface OverflowData {
  reason: "boot-changed" | "ring-evicted" | "lagged";
  boot: string;
  next_seq: number;
  oldest?: number;
  build?: string;
}

const handlers = new Map<string, Set<Handler>>();
let streamAbort: AbortController | null = null;
let started = false;
let lastFrameAt = 0;
let activityRevision = 0;
let hasAttemptedConnection = false;
let reconnectState: WebReconnectState = "connected";
let cancelReconnectWait: (() => void) | null = null;
let wakeReconnectWait: (() => void) | null = null;
let streamGeneration = 0;
let wakeRequested = false;
let receiveDeadline: ReturnType<typeof setTimeout> | null = null;
let streamRelayed = false;
let silentProof: Promise<void> | null = null;

type ConnectionStage = "attempt" | "ready" | "activity" | "frame" | "silence" | "proof-ok" | "proof-failed" | "channel-replace" | "lost" | "retry" | "wake";
const connectionDiagnostics: object[] = [];
declare global { interface Window { __ctConnectionDiagnostics?: () => readonly object[] } }

function diagnose(stage: ConnectionStage, end?: StreamEnd, retryMs?: number): void {
  if (!DEV_BUILD) return;
  window.__ctConnectionDiagnostics ??= webConnectionDiagnostics;
  const record = {
    at: Date.now(), stage, generation: streamGeneration, state: reconnectState,
    hidden: document.hidden, silentMs: Date.now() - lastFrameAt, relayed: streamRelayed,
    cursor: cursor !== null, end: end?.reason,
    status: end?.reason === "http" ? end.status : undefined,
    error: end?.error instanceof Error ? end.error.name : undefined, retryMs,
    mux: streamMuxDiagnostics().map(({ requests, streams, queued, bufferedBytes, carrier, connected }) =>
      ({ requests, streams, queued, bufferedBytes, carrier, connected })),
  };
  if (connectionDiagnostics.length === 128) connectionDiagnostics.shift();
  connectionDiagnostics.push(record);
  console.debug("[event-connection]", record);
}

export function webConnectionDiagnostics(): readonly object[] { return DEV_BUILD ? [...connectionDiagnostics] : []; }

export function wakeWebEvents(): void {
  diagnose("wake");
  wakeStreamMuxes();
  if (wakeReconnectWait) {
    wakeReconnectWait();
  } else if (reconnectState === "connected" && Date.now() - lastFrameAt > RECEIVE_TIMEOUT_MS) {
    if (streamAbort) void proveSilentStream(streamAbort, streamGeneration);
  }
}

function proveSilentStream(abort: AbortController, generation: number): Promise<void> {
  if (silentProof) return silentProof;
  const pendingProof = checkSilentStream(abort, generation).finally(() => {
    if (silentProof === pendingProof) silentProof = null;
  });
  silentProof = pendingProof;
  return pendingProof;
}

async function checkSilentStream(abort: AbortController, generation: number): Promise<void> {
  const observedActivity = activityRevision;
  diagnose("silence");
  const alive = await verifyStreamMuxes(eventOrigin());
  if (abort.signal.aborted || generation !== streamGeneration) return;
  if (activityRevision !== observedActivity) {
    diagnose("proof-ok");
    armReceiveDeadline(abort);
  } else {
    diagnose(alive ? "channel-replace" : "proof-failed");
    wakeRequested = true;
    abort.abort(new DOMException(alive ? "Event channel stopped delivering keepalives" : "Carrier liveness proof failed", "TimeoutError"));
  }
}

function armReceiveDeadline(abort: AbortController): void {
  if (receiveDeadline) clearTimeout(receiveDeadline);
  const generation = streamGeneration;
  receiveDeadline = setTimeout(() => {
    receiveDeadline = null;
    void proveSilentStream(abort, generation);
  }, RECEIVE_TIMEOUT_MS);
  (receiveDeadline as unknown as { unref?: () => void }).unref?.();
}

function waitForReconnectFailure(delay: number, hiddenDelay: number, generation: number): Promise<boolean> {
  reconnectState = transitionWebReconnect(reconnectState, "lost").state;
  return new Promise((resolve) => {
    const startedAt = Date.now();
    let deadlineAt = startedAt + delay;
    let deadline: ReturnType<typeof setTimeout> | null = null;
    const scheduleDeadline = () => {
      if (deadline !== null) clearTimeout(deadline);
      deadline = setTimeout(() => finish("deadline"), Math.max(0, deadlineAt - Date.now()));
      (deadline as unknown as { unref?: () => void }).unref?.();
    };
    const finish = (event: "wake" | "deadline" | "stop") => {
      if (deadline !== null) {
        clearTimeout(deadline);
        deadline = null;
      }
      document.removeEventListener("visibilitychange", onVisibilityChange);
      cancelReconnectWait = null;
      wakeReconnectWait = null;
      const next = transitionWebReconnect(reconnectState, event);
      reconnectState = next.state;
      resolve(generation === streamGeneration && next.action === "probe");
    };
    const onVisibilityChange = () => {
      if (!document.hidden) finish("wake");
      else {
        deadlineAt = Math.max(deadlineAt, startedAt + hiddenDelay);
        scheduleDeadline();
      }
    };
    scheduleDeadline();
    document.addEventListener("visibilitychange", onVisibilityChange);
    wakeReconnectWait = () => finish("wake");
    cancelReconnectWait = () => finish("stop");
    window.dispatchEvent(new Event(TERMINAL_VIEWER_DISCONNECTED_EVENT));
  });
}

const CURSOR_KEY = "codeterm-web-events-cursor";
const RECEIVE_TIMEOUT_MS = 30_000;

// refetches regardless); mux state uses its own seam below, rest need per-hook wiring.
export const RESYNC_EVENTS = [
  "mesh-state-changed",
  "delayed-send-changed",
  "service-health-changed",
];

function readCursorTarget(): string | null {
  try {
    return cursorKey();
  } catch {
    return null;
  }
}

function readCursor(): string | null {
  try {
    return sessionStorage.getItem(cursorKey());
  } catch {
    return null;
  }
}

function cursorKey(): string {
  return `${CURSOR_KEY}:${eventOrigin()}:${relayTarget()?.id ?? "self"}`;
}

let cursorTarget = readCursorTarget();
let cursor: string | null = readCursor();
const seqGaps = createSeqGapTracker();


function saveCursor(next: string): void {
  cursor = next;
}

function persistCursor(): void {
  if (cursorTarget === null || cursor === null) return;
  try {
    sessionStorage.setItem(cursorTarget, cursor);
  } catch {
    /* storage unavailable (private mode) — memory-only cursor still works */
  }
}

function persistHiddenCursor(): void {
  if (document.hidden) persistCursor();
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

// refetch reachable state; coverage beyond this is tracked as follow-up.
function runFullResync(info: OverflowData): void {
  // as "last seen" means a later reconnect replays from next_seq forward.
  if (Number.isSafeInteger(info.next_seq)) {
    saveCursor(`${info.boot}:${info.next_seq - 1}`);
    seqGaps.adopt(info.boot, info.next_seq - 1);
  }
  window.dispatchEvent(new Event("codeterm-refresh-mux-state"));
  for (const ev of RESYNC_EVENTS) dispatch(ev, null);
  fireResync(info.reason === "boot-changed" ? "epoch_changed" : "overflow");
  if (info.reason === "boot-changed") {
    // and needs revalidation after a daemon restart (usePtyConnection.ts).
    const detail: BootChangedDetail = { boot: info.boot, relayed: streamRelayed };
    if (typeof info.build === "string") detail.build = info.build;
    window.dispatchEvent(new CustomEvent(WEB_BOOT_CHANGED_EVENT, { detail }));
  }
}

function handleFrame(msg: EventFrame): void {
  if (msg.event_type === "overflow") {
    runFullResync(msg.data as OverflowData);
    return;
  }
  if (msg.boot && typeof msg.seq === "number") {
    const seen = seqGaps.observe(msg.boot, msg.seq, msg.prev_seq);
    if (seen.verdict === "gap") {
      // Loss with no overflow frame: a contract violation, not a hiccup.
      console.warn(
        `[event-seq-gap] expected ${seen.expected} got ${msg.seq} (boot ${msg.boot})`,
      );
      runFullResync({ reason: "ring-evicted", boot: msg.boot, next_seq: msg.seq });
      // Because this frame is already `next_seq`, rebaseline past it or the following frame appears to be a gap.
      seqGaps.adopt(msg.boot, msg.seq);
    }
    if (seen.verdict === "duplicate") return;
    saveCursor(`${msg.boot}:${msg.seq}`);
  }
  dispatch(msg.event_type, msg.data);
}

export function webEventStreamUrl(
  replayCursor: string | null,
  target: { id: string; name: string } | null = relayTarget(),
): string {
  if (!target) {
    const since = replayCursor ? `?since=${encodeURIComponent(replayCursor)}` : "";
    return `${eventOrigin()}/api/events/stream${since}`;
  }
  const params = new URLSearchParams({
    peer: target.id,
    path: "/api/events/stream",
  });
  if (replayCursor) params.set("since", replayCursor);
  return `${eventOrigin()}/api/mesh/relay-stream?${params.toString()}`;
}

async function runStream(): Promise<void> {
  const generation = ++streamGeneration;
  const local = isLocalEventStream(eventOrigin(), relayTarget() !== null);
  const firstBackoff = local ? LOCAL_EVENT_RETRY_MS : RECONNECT_FIRST_MS;
  let backoffMs = firstBackoff;
  for (;;) {
    if (generation !== streamGeneration) return;
    const attemptStartedAt = Date.now();
    const abort = new AbortController();
    streamAbort = abort;
    lastFrameAt = Date.now();
    armReceiveDeadline(abort);
    const reconnecting = hasAttemptedConnection;
    hasAttemptedConnection = true;
    let connected = false;
    let sawApplicationFrame = false;
    let end: StreamEnd | undefined;
    const client = eventClient ?? baseDaemonTransport();
    const markConnected = (stage: "ready" | "activity" | "frame") => {
      if (abort.signal.aborted || generation !== streamGeneration || connected) return;
      connected = true;
      backoffMs = firstBackoff;
      reconnectState = transitionWebReconnect(reconnectState, "connected").state;
      diagnose(stage);
      if (reconnecting) {
        window.dispatchEvent(new Event(TERMINAL_VIEWER_RECONNECTED_EVENT));
        window.dispatchEvent(new Event("codeterm-refresh-mux-state"));
      }
    };
    diagnose("attempt");
    if (cursor) {
      const separator = cursor.lastIndexOf(":");
      const seq = Number(cursor.slice(separator + 1));
      if (separator > 0 && Number.isSafeInteger(seq) && seq >= 0) seqGaps.adopt(cursor.slice(0, separator), seq);
    }
    const target = relayTarget();
    await new Promise<void>((resolve) => {
      client.subscribe(webEventStreamUrl(cursor, target).slice(client.origin().length), (_name, data) => {
        if (abort.signal.aborted || generation !== streamGeneration) return;
        streamRelayed = target !== null;
        const frame = data as EventFrame;
        if (!frame || typeof frame.event_type !== "string") return;
        const firstFrame = !sawApplicationFrame;
        sawApplicationFrame = true;
        markConnected("frame");
        handleFrame(frame);
        if (!firstFrame && frame.event_type === "overflow") signalEventStreamReady();
      }, {
        signal: abort.signal, raw: true,
        onStarting() {
          if (abort.signal.aborted || generation !== streamGeneration) return;
          if (receiveDeadline) { clearTimeout(receiveDeadline); receiveDeadline = null; }
        },
        onReady() {
          if (abort.signal.aborted || generation !== streamGeneration) return;
          markConnected("ready");
          armReceiveDeadline(abort);
          signalEventStreamReady();
          if (reconnecting) window.dispatchEvent(new Event(WEB_EVENTS_READY_EVENT));
        },
        onActivity() {
          if (abort.signal.aborted || generation !== streamGeneration) return;
          lastFrameAt = Date.now();
          activityRevision += 1;
          markConnected("activity");
          armReceiveDeadline(abort);
        },
        onEnd(outcome) {
          end = outcome;
          resolve();
        },
      });
    });
    if (generation !== streamGeneration) return;
    if (shouldResetReconnectBackoff(connected, Date.now() - attemptStartedAt)) backoffMs = firstBackoff;
    diagnose("lost", end);
    signalEventStreamLost();
    abort.abort();
    if (receiveDeadline) { clearTimeout(receiveDeadline); receiveDeadline = null; }
    if (wakeRequested) {
      wakeRequested = false;
      continue;
    }
    const hidden = document.hidden;
    const random = Math.random();
    const delay = eventReconnectDelay(backoffMs, random, hidden, local);
    const hiddenDelay = eventReconnectDelay(backoffMs, random, true, local);
    diagnose("retry", end, delay);
    if (!await waitForReconnectFailure(delay, hiddenDelay, generation)) return;
    backoffMs = nextEventReconnectBackoff(backoffMs, document.hidden, local);
  }
}

function ensureStarted(): void {
  if (started) return;
  started = true;
  window.addEventListener("pagehide", persistCursor);
  document.addEventListener("visibilitychange", persistHiddenCursor);
  void runStream();
}

/** Silent seq discontinuities seen this page lifetime. Non-zero means the
 *  daemon dropped events without an overflow frame. */
export function webEventSeqGaps(): number {
  return seqGaps.gaps();
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
  persistCursor();
  stopReconnect();
  hasAttemptedConnection = true;
  seqGaps.reset();
  const nextTarget = cursorKey();
  cursor = replayCursorForTarget(cursorTarget, cursor, nextTarget, readCursor());
  cursorTarget = nextTarget;
  started = false;
  streamAbort?.abort();
  streamAbort = null;
  ensureStarted();
}

function stopReconnect(): void {
  ++streamGeneration;
  cancelReconnectWait?.();
  if (receiveDeadline) clearTimeout(receiveDeadline);
  receiveDeadline = null;
  reconnectState = "connected";
  wakeRequested = false;
  silentProof = null;
  hasAttemptedConnection = false;
}

export function __resetWebEventsForTest(): void {
  window.removeEventListener("pagehide", persistCursor);
  document.removeEventListener("visibilitychange", persistHiddenCursor);
  stopReconnect();
  seqGaps.reset();
  started = false;
  streamAbort?.abort();
  streamAbort = null;
  handlers.clear();
  cursorTarget = readCursorTarget();
  cursor = readCursor();
  lastFrameAt = 0;
  hasAttemptedConnection = false;
}
