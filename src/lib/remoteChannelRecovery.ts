import type { RemoteTransportAvailability, RemoteTransportChannel } from "./remoteTabTransportState";

export type StreamEndReason = "eof" | "error";
export type RecoveryDecision = "reconnect" | "wait" | "stale";

export interface RemoteChannelRecoveryOptions<C> {
  channel: RemoteTransportChannel;
  // Strictly-ahead check: true => `next` is forward progress; false => duplicate/stale frame.
  advanced: (prev: C | null, next: C) => boolean;
  onAvailability?: (channel: RemoteTransportChannel, availability: RemoteTransportAvailability) => void;
}

export interface RemoteChannelRecovery<C> {
  readonly channel: RemoteTransportChannel;
  generation(): number;
  currentCursor(): C | null;
  availability(): RemoteTransportAvailability;
  isAwaitingSignal(): boolean;
  isCurrent(generation: number): boolean;
  beginConnection(): number;
  noteProgress(generation: number, cursor: C): boolean;
  handleStreamEnd(generation: number, reason: StreamEndReason): RecoveryDecision;
  signal(): RecoveryDecision;
  dispose(): void;
}

// PTY-agnostic recovery state machine shared by output-SSE and chat-SSE consumers.
// Owns a monotonic per-subscription generation, an opaque resume cursor, duplicate
// suppression, generation-safe gating, and the D1 reconnect policy. No timers, no
// sleeps, no busy-loop: reconnects are event-driven and caller-initiated.
export function createRemoteChannelRecovery<C>(
  options: RemoteChannelRecoveryOptions<C>,
): RemoteChannelRecovery<C> {
  let generation = 0;
  let cursor: C | null = null;
  let progressInActive = false;
  let awaitingSignal = false;
  let disposed = false;
  // Starts optimistically 'available' to match the fresh projection default, so the
  // first real transition (a zero-progress/error end -> 'reconnecting') is not deduped.
  let state: RemoteTransportAvailability = "available";

  function emit(next: RemoteTransportAvailability): void {
    if (state === next) return;
    state = next;
    options.onAvailability?.(options.channel, next);
  }

  const isCurrent = (g: number): boolean => !disposed && g === generation;

  return {
    channel: options.channel,
    generation: () => generation,
    currentCursor: () => cursor,
    availability: () => state,
    isAwaitingSignal: () => awaitingSignal,
    isCurrent,
    beginConnection() {
      generation += 1;
      progressInActive = false;
      awaitingSignal = false;
      return generation;
    },
    noteProgress(g, next) {
      if (!isCurrent(g)) return false; // G5: a superseded generation never mutates cursor/state.
      if (!options.advanced(cursor, next)) return false; // duplicate/regressed frame dropped.
      cursor = next;
      progressInActive = true;
      emit("available");
      return true;
    },
    handleStreamEnd(g, reason) {
      if (disposed || g !== generation) return "stale"; // stale/superseded end ignored.
      // D1: a clean EOF reconnects immediately from the cursor ONLY when the stream
      // made progress. Re-arms on every such end — never one-shot.
      if (reason === "eof" && progressInActive) return "reconnect";
      // Zero-progress EOF or any connection error: go 'reconnecting' and wait for an
      // explicit signal (mesh-state-changed/resync). This is the no-busy-loop guard.
      awaitingSignal = true;
      emit("reconnecting");
      return "wait";
    },
    signal() {
      if (disposed) return "stale";
      awaitingSignal = false;
      return "reconnect";
    },
    dispose() {
      disposed = true;
      awaitingSignal = false;
    },
  };
}

// Two peer keep-alive intervals (15s) plus slack: a remote SSE stream silent this
// long has almost certainly lost its path. Shared by the remote silence deadline
// and the local output-stream watchdog so the two never drift apart.
export const REMOTE_SILENCE_TIMEOUT_MS = 30_000;

export function parkStalledConnection<C>(
  recovery: RemoteChannelRecovery<C>,
  generation: number,
  controller: AbortController,
): boolean {
  if (!recovery.isCurrent(generation)) return false;
  // Explicit park: the chat finalizer short-circuits on abort BEFORE handleStreamEnd,
  // so relying on abort alone would strand the chat channel. Idempotent for output,
  // whose finalizer also parks.
  recovery.handleStreamEnd(generation, "error");
  controller.abort();
  return true;
}

export interface SilenceDeadline {
  arm(): void;
  clear(): void;
}

// A remote SSE connection can go silent after a 200+headers — before its first
// byte, or after a keep-alive or two on an empty-ring tab (a dropped path
// mid-replay): reader.read() never resolves, the stream never ends, the channel
// never parks, and resume then skips a cursor-less connection forever. This is the
// only liveness mechanism for that window. Re-armed on open and on EVERY byte, so
// expiry means "no bytes for the whole window" — a keep-alive re-arms an idle-but-
// healthy tab, only true silence trips it. Elapsed time is a failure deadline here,
// never readiness. clear() on teardown so a re-arming chain cannot outlive its
// connection.
export function createSilenceDeadline<C>(
  recovery: RemoteChannelRecovery<C>,
  generation: number,
  controller: AbortController,
  timeoutMs: number = REMOTE_SILENCE_TIMEOUT_MS,
): SilenceDeadline {
  let timer: ReturnType<typeof setTimeout> | null = null;
  const clear = (): void => {
    if (timer === null) return;
    clearTimeout(timer);
    timer = null;
  };
  const arm = (): void => {
    clear();
    timer = setTimeout(() => {
      timer = null;
      parkStalledConnection(recovery, generation, controller);
    }, timeoutMs);
    (timer as unknown as { unref?: () => void }).unref?.(); // a background deadline must not hold the runtime
  };
  return { arm, clear };
}
