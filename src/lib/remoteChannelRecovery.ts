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
  let state: RemoteTransportAvailability = "reconnecting";

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
