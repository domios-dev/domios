/**
 * Multiplexed web PTY output (D1 §5): ONE SSE connection carries every pane's
 * output as pane-tagged frames (`GET /api/output/stream?since=<b64url cursor>`),
 * so the browser holds O(1) connections regardless of pane count — replacing
 * the per-pane 25s long-poll.
 *
 * Cursor = `{boot, panes: {paneId: byteOffset}}`. The server resumes listed
 * panes at their offset and full-replays unlisted ones, so after the first
 * connect we track offsets for EVERY pane frame we see (sink or not) to keep
 * reconnects exact. A `boot_changed` frame (daemon restart, or first cold
 * connect with an unknown boot) clears offsets and reconnects — the fresh
 * cursor then full-replays the new ring.
 *
 * Frames: `{pane_id, output, offset}` (lossy-UTF8 text; offset = resume point),
 * `{pane_id, truncated: true, from}` (ring evicted while behind),
 * `{pane_id, ended: true}` (genuine PTY death), `{boot_changed: true, boot}`.
 *
 * Fallback: the per-pane long-poll path stays behind `outputStreamEnabled()`
 * (localStorage `codeterm-web-output-stream` = "off").
 *
 * Relay: with a mesh relay target set the stream is byte-piped through the local
 * daemon's `/api/mesh/relay-stream` to the peer's own `/api/output/stream`
 * (D2 §3) — same frames, same cursor, no relay-specific loss.
 */
import { apiHost, currentToken, IS_WEB, relayTarget } from "./webApi";
import { onResync } from "./resync";

export interface PaneOutputSink {
  onOutput(text: string): void;
  onTruncated(from: number): void;
  onEnded(): void;
}

interface OutputFrame {
  pane_id?: string;
  output?: string;
  offset?: number;
  truncated?: boolean;
  from?: number;
  ended?: boolean;
  boot_changed?: boolean;
  boot?: string;
}

const BOOT_KEY = "codeterm-web-output-boot";
const RECONNECT_DEBOUNCE_MS = 50;
const RECEIVE_TIMEOUT_MS = 30_000;
const WATCHDOG_INTERVAL_MS = 5_000;

declare global {
  interface Window {
    __outputStreamPerf?: {
      frames: number;
      bytes: number;
      batches: number;
      lastFrameAt: number;
    };
  }
}

const sinks = new Map<string, Set<PaneOutputSink>>();
const offsets = new Map<string, number>();
/** Panes whose full history was already streamed into a live xterm this page
 *  lifetime — a re-mount must NOT re-request a multi-MB replay. */
const replayed = new Set<string>();

let boot: string | null = readBoot();
let streamAbort: AbortController | null = null;
let watchdogTimer: ReturnType<typeof setInterval> | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let started = false;
let lastFrameAt = 0;
let forceImmediateRetry = false;

function readBoot(): string | null {
  try {
    return sessionStorage.getItem(BOOT_KEY);
  } catch {
    return null;
  }
}

function saveBoot(b: string): void {
  boot = b;
  try {
    sessionStorage.setItem(BOOT_KEY, b);
  } catch {
    /* private mode — memory-only boot still works */
  }
}

/** Manual killswitch back to the per-pane long-poll transport. */
export function outputStreamEnabled(): boolean {
  try {
    return localStorage.getItem("codeterm-web-output-stream") !== "off";
  } catch {
    return true;
  }
}

/** True when this pane's output should come from the multiplexed stream.
 *  Relay targets now stream too (D2 §3): the local daemon byte-pipes the peer's
 *  own `/api/output/stream?since=`, so relayed cursors inherit no-loss (S2c). */
export function webOutputStreamActive(): boolean {
  return IS_WEB && outputStreamEnabled();
}

function sinceCursor(): string {
  const panes: Record<string, number> = {};
  for (const [pane, off] of offsets) panes[pane] = off;
  const json = JSON.stringify({ boot: boot ?? "", panes });
  return btoa(String.fromCharCode(...new TextEncoder().encode(json)))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

function outputStreamUrl(): string {
  const cursor = sinceCursor();
  const target = relayTarget();
  if (target) {
    const params = new URLSearchParams({
      peer: target.id,
      path: "/api/output/stream",
      since: cursor,
    });
    return `${apiHost()}/api/mesh/relay-stream?${params.toString()}`;
  }
  return `${apiHost()}/api/output/stream?since=${cursor}`;
}

function handleFrame(msg: OutputFrame): void {
  const perf = window.__outputStreamPerf;
  if (perf) {
    perf.frames += 1;
    perf.lastFrameAt = performance.now();
    if (typeof msg.output === "string") perf.bytes += msg.output.length;
  }
  if (msg.boot_changed && msg.boot) {
    // boot-scoped offsets and reconnect — the boot-matched cursor full-replays.
    saveBoot(msg.boot);
    offsets.clear();
    forceReconnect();
    return;
  }
  const pane = msg.pane_id;
  if (!pane) return;
  const set = sinks.get(pane);
  if (typeof msg.offset === "number" && typeof msg.output === "string") {
    offsets.set(pane, msg.offset);
    if (set) {
      perf && (perf.batches += 1);
      for (const s of set) s.onOutput(msg.output);
    }
    return;
  }
  if (msg.truncated && typeof msg.from === "number") {
    if (set) for (const s of set) s.onTruncated(msg.from);
    return;
  }
  if (msg.ended) {
    if (set) for (const s of set) s.onEnded();
  }
}

function forceReconnect(): void {
  if (reconnectTimer) return;
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    forceImmediateRetry = true;
    streamAbort?.abort();
  }, RECONNECT_DEBOUNCE_MS);
}

async function runStream(): Promise<void> {
  let backoffMs = 1_000;
  for (;;) {
    const abort = new AbortController();
    streamAbort = abort;
    lastFrameAt = Date.now();
    try {
      const token = currentToken();
      const res = await fetch(outputStreamUrl(), {
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
          lastFrameAt = Date.now(); // keep-alive comments count as liveness too
          const dataLine = frame.split("\n").find((l) => l.startsWith("data:"));
          if (!dataLine) continue;
          try {
            handleFrame(JSON.parse(dataLine.slice(5).trim()) as OutputFrame);
          } catch {
            /* malformed frame — skip */
          }
        }
      }
    } catch {
      /* connection lost / aborted / reconnect-forced — retry below */
    }
    if (abort.signal.aborted && streamAbort !== abort) return; // superseded
    if (forceImmediateRetry) {
      forceImmediateRetry = false;
      backoffMs = 1_000;
      continue;
    }
    const delay = backoffMs / 2 + Math.random() * (backoffMs / 2);
    await new Promise((r) => setTimeout(r, delay));
    backoffMs = Math.min(backoffMs * 2, 15_000);
  }
}

function ensureStarted(): void {
  if (started) return;
  started = true;
  // Small delay so simultaneous terminal mounts batch into one initial cursor.
  setTimeout(() => void runStream(), RECONNECT_DEBOUNCE_MS);
  watchdogTimer ??= setInterval(() => {
    if (Date.now() - lastFrameAt > RECEIVE_TIMEOUT_MS) {
      forceImmediateRetry = true;
      streamAbort?.abort();
    }
  }, WATCHDOG_INTERVAL_MS);
}

/**
 * Attach a terminal to a pane's multiplexed output. First subscription of a
 * pane this page-lifetime requests a full history replay (offset 0); re-mounts
 * resume from the tracked offset (xterm is keep-alive, scrollback persists).
 */
export function subscribePaneOutput(paneId: string, sink: PaneOutputSink): () => void {
  let set = sinks.get(paneId);
  if (!set) {
    set = new Set();
    sinks.set(paneId, set);
  }
  set.add(sink);
  if (!replayed.has(paneId)) {
    replayed.add(paneId);
    offsets.set(paneId, 0);
    if (started) forceReconnect();
  }
  ensureStarted();
  return () => {
    set!.delete(sink);
  };
}

/** Force an immediate reconnect keeping all offsets (wake-from-sleep resume):
 *  the fresh `?since` cursor replays exactly what was missed. */
export function pokeOutputStream(): void {
  if (!started) return;
  forceImmediateRetry = true;
  streamAbort?.abort();
}

onResync(() => pokeOutputStream());

/** Tear down (peer switch) — next subscription reconnects to the current host. */
export function resetOutputStream(): void {
  if (watchdogTimer) {
    clearInterval(watchdogTimer);
    watchdogTimer = null;
  }
  if (reconnectTimer) {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }
  started = false;
  streamAbort?.abort();
  streamAbort = null;
  offsets.clear();
  replayed.clear();
  if (sinks.size > 0) ensureStarted();
}
