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

// Recovery is event-driven and caller-initiated, with per-subscription generation and cursor state; it uses no timers or sleeps.
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

// Use one deadline for remote silence and the local output watchdog so they cannot drift apart.
export const REMOTE_SILENCE_TIMEOUT_MS = 30_000;

export function parkStalledConnection<C>(
  recovery: RemoteChannelRecovery<C>,
  generation: number,
  controller: AbortController,
): boolean {
  if (!recovery.isCurrent(generation)) return false;
  // Abort handling must explicitly park the chat channel because its finalizer bypasses `handleStreamEnd`.
  recovery.handleStreamEnd(generation, "error");
  controller.abort();
  return true;
}

export interface SilenceDeadline {
  arm(): void;
  clear(): void;
}

export interface SilenceDeadlineScheduler {
  schedule(callback: () => void, timeoutMs: number): unknown;
  cancel(handle: unknown): void;
}

const systemSilenceDeadlineScheduler: SilenceDeadlineScheduler = {
  schedule(callback, timeoutMs) {
    const timer = setTimeout(callback, timeoutMs);
    (timer as unknown as { unref?: () => void }).unref?.();
    return timer;
  },
  cancel(handle) {
    clearTimeout(handle as ReturnType<typeof setTimeout>);
  },
};

// Re-arm the deadline on open and each byte so only sustained silence parks the channel, and clear it on teardown.
export function createSilenceDeadline<C>(
  recovery: RemoteChannelRecovery<C>,
  generation: number,
  controller: AbortController,
  timeoutMs: number = REMOTE_SILENCE_TIMEOUT_MS,
  scheduler: SilenceDeadlineScheduler = systemSilenceDeadlineScheduler,
): SilenceDeadline {
  let timer: unknown | null = null;
  const clear = (): void => {
    if (timer === null) return;
    scheduler.cancel(timer);
    timer = null;
  };
  const arm = (): void => {
    clear();
    timer = scheduler.schedule(() => {
      timer = null;
      parkStalledConnection(recovery, generation, controller);
    }, timeoutMs);
  };
  return { arm, clear };
}
