/**
 * The web mirror, without a browser.
 *
 * Runs the real `src/lib/webEvents` SSE client and the real multiplexer
 * reducer against a live dev daemon, wired the way `src/store/multiplexer`
 * wires them: `codeterm-refresh-mux-state` and a snapshot-shaped
 * `mux-state-changed` both land as `SET_STATE`.
 *
 * The event layer and the reducer are the product's own modules. The dozen
 * lines of orchestration between them (`MultiplexerProvider`'s effect) are
 * modelled here rather than imported, because that module is a React
 * component: `looksLikeSnapshot` mirrors `muxStateEventAction`, and
 * `src/store/multiplexer/reducer.test.ts` covers the real store directly.
 *
 * Every wait here is a wait on an observable frame. There is no sleep, no
 * poll interval, and no fixed delay: `awaitFrame` resolves when the daemon's
 * own event for the action arrives. Deadlines are failure bounds only.
 */
import type { Action } from "../../src/store/multiplexer/reducer";
import type { ExtendedState } from "../../src/store/multiplexer/tree";
import type { MultiplexerState } from "../../src/lib/types";

export interface FrameRecord {
  event_type: string;
  data: unknown;
}

export interface MirrorCounters {
  frames: number;
  /** `overflow` never reaches a handler - webEvents swallows it into
   *  runFullResync. Its only external signature is the null-payload
   *  re-dispatch of every RESYNC_EVENT, so one overflow == one such round. */
  resyncFanout: number;
  muxRefetches: number;
  /** A refetch that never returned a snapshot. The store cannot advance on one,
   *  and nothing in the product retries it - so it is a divergence path. */
  refetchErrors: number;
  /** Peak concurrent snapshot refetches. Separates a client-side pile-up
   *  from a daemon that simply outruns any single consumer. */
  maxInFlight: number;
  /** Slowest single refetch, ms. */
  refetchMaxMs: number;
}

export interface Mirror {
  counters: MirrorCounters;
  gate: import("./webEnv").FetchGate;
  /** Overflow frames observed, derived from the resync fan-out. */
  overflows: () => number;
  /** `boot-changed` overflows specifically. */
  bootChanges: () => number;
  muxRefreshes: () => number;
  state: () => ExtendedState;
  awaitFrame: (match: (frame: FrameRecord) => boolean, deadlineMs: number, label: string) => Promise<FrameRecord>;
  /** Resolves once no snapshot refetch is in flight - the store has applied
   *  everything it asked for. An event-count wait, not a delay. */
  settle: (deadlineMs: number) => Promise<void>;
  /** Resolves once the SSE transport has answered with headers. `minOpens`
   *  is the connection count to wait past, so a caller can wait for the NEXT
   *  connection rather than be satisfied by one that is already gone. */
  awaitStreamOpen: (deadlineMs: number, minOpens?: number) => Promise<void>;
  /** Connections the event stream has opened so far. */
  streamOpens: () => number;
  /** Replays WebPeerPicker.switchTo's sequence for an origin switch. The
   *  picker is a React component, so the sequence is modelled here; its steps
   *  and their order are read from WebPeerPicker.tsx and listed in FINDINGS
   *  4b. It deliberately does NOT call resetOutputStream(), because the picker
   *  does not either - that omission is what prediction P2 tests. */
  switchOrigin: (peer: { id: string; name: string } | null) => void;
  stop: () => void;
}

export interface MirrorOptions {
  origin: string;
  token: string;
  relayPeer?: { id: string; name: string } | null;
}

/** Mirrors `muxStateEventAction`: a payload carrying `workspaces` is a snapshot. */
function looksLikeSnapshot(payload: unknown): boolean {
  return Boolean(
    payload &&
      typeof payload === "object" &&
      Array.isArray((payload as { workspaces?: unknown }).workspaces),
  );
}

/** Refuses to run against prod. The bench lib guards the CLI; this guards the harness. */
export function assertDevOrigin(origin: string): void {
  const port = new URL(origin).port;
  if (port === "7685") {
    throw new Error(`refusing to target prod on 7685 (origin=${origin})`);
  }
  if (port !== "7686") {
    throw new Error(`expected the dev port 7686, got ${port} (origin=${origin})`);
  }
}

export async function startMirror(options: MirrorOptions): Promise<Mirror> {
  assertDevOrigin(options.origin);
  const { installWebEnv } = await import("./webEnv");
  const env = installWebEnv(options);

  const webEvents = await import("../../src/lib/webEvents");
  const { reducer } = await import("../../src/store/multiplexer/reducer");
  const {
    webJson, setRelayTarget, clearRelayTarget, invalidateWebRequestContext,
  } = await import("../../src/lib/webApi");

  const counters: MirrorCounters = {
    frames: 0, resyncFanout: 0, muxRefetches: 0, refetchErrors: 0,
    maxInFlight: 0, refetchMaxMs: 0,
  };

  let state: ExtendedState = {
    workspaces: [],
    tabs: {},
    teams: [],
    active_workspace_id: null,
    focused_tab_id: null,
    notifications: [],
    tabStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 0,
    pendingCloses: [],
  };
  let notifyWaiters = (): void => {};
  const apply = (action: Action): void => {
    state = reducer(state, action);
    notifyWaiters();
  };

  const IDLE: FrameRecord = { event_type: "__store_changed", data: null };
  const waiters = new Set<{ match: (f: FrameRecord) => boolean; resolve: (f: FrameRecord) => void }>();
  const wake = (frame: FrameRecord): void => {
    for (const waiter of [...waiters]) {
      if (waiter.match(frame)) {
        waiters.delete(waiter);
        waiter.resolve(frame);
      }
    }
  };
  notifyWaiters = () => wake(IDLE);
  const observe = (frame: FrameRecord): void => {
    counters.frames++;
    wake(frame);
  };

  let inFlight = 0;
  const idleWaiters = new Set<() => void>();
  const refetch = async (): Promise<void> => {
    counters.muxRefetches++;
    inFlight++;
    if (inFlight > counters.maxInFlight) counters.maxInFlight = inFlight;
    const startedAt = Date.now();
    try {
      const snapshot = await webJson<MultiplexerState>("GET", "/api/mux/state");
      if (snapshot) apply({ type: "SET_STATE", state: snapshot });
    } catch {
      counters.refetchErrors++;
    } finally {
      const took = Date.now() - startedAt;
      if (took > counters.refetchMaxMs) counters.refetchMaxMs = took;
      inFlight--;
      if (inFlight === 0) {
        for (const resolveIdle of [...idleWaiters]) resolveIdle();
        idleWaiters.clear();
      }
    }
  };

  env.onMuxRefresh(() => void refetch());

  const unlisteners: Array<() => void> = [];
  // Every event the mirror consumes goes through the same observation seam, so
  // a wait is always a wait on a real daemon frame.
  for (const eventType of ["mux-state-changed", "tab-focused", "tab-closed", "tab-title-pinned"]) {
    unlisteners.push(
      webEvents.webListen(eventType, (e) => {
        observe({ event_type: eventType, data: e.payload });
        if (eventType !== "mux-state-changed") return;
        if (looksLikeSnapshot(e.payload)) {
          apply({ type: "SET_STATE", state: e.payload as MultiplexerState });
        } else {
          void refetch();
        }
      }),
    );
  }
  for (const eventType of webEvents.RESYNC_EVENTS) {
    unlisteners.push(
      webEvents.webListen(eventType, (e) => {
        if (e.payload === null) counters.resyncFanout++;
        observe({ event_type: eventType, data: e.payload });
      }),
    );
  }

  return {
    counters,
    gate: env.gate,
    overflows: () => Math.round(counters.resyncFanout / webEvents.RESYNC_EVENTS.length),
    bootChanges: env.bootChanges,
    muxRefreshes: env.muxRefreshes,
    state: () => state,
    awaitFrame(match, deadlineMs, label) {
      return new Promise<FrameRecord>((resolve, reject) => {
        const waiter = { match, resolve: (f: FrameRecord) => { clearTimeout(deadline); resolve(f); } };
        // Failure deadline only - expiry is never read as readiness.
        const deadline = setTimeout(() => {
          waiters.delete(waiter);
          reject(new Error(`deadline: no frame matched "${label}" within ${deadlineMs}ms`));
        }, deadlineMs);
        waiters.add(waiter);
      });
    },
    switchOrigin(peer) {
      invalidateWebRequestContext();
      apply({ type: "RESET_PEER_CONTEXT" });
      if (peer) setRelayTarget(peer.id, peer.name);
      else clearRelayTarget();
      invalidateWebRequestContext();
      webEvents.resetWebEvents();
      void refetch();
    },
    streamOpens: () => env.gate.opens(),
    awaitStreamOpen(deadlineMs, minOpens = 1) {
      if (env.gate.opens() >= minOpens) return Promise.resolve();
      return new Promise<void>((resolve, reject) => {
        const deadline = setTimeout(
          () => reject(new Error(
            `deadline: event stream did not reach ${minOpens} open(s) within ` +
            `${deadlineMs}ms (${env.gate.attempts()} attempt(s), ` +
            `${env.gate.opens()} open(s))`,
          )),
          deadlineMs,
        );
        env.gate.onOpen(() => {
          if (env.gate.opens() < minOpens) return;
          clearTimeout(deadline);
          resolve();
        });
      });
    },
    settle(deadlineMs) {
      if (inFlight === 0) return Promise.resolve();
      return new Promise<void>((resolve, reject) => {
        const wake = () => { clearTimeout(deadline); resolve(); };
        const deadline = setTimeout(() => {
          idleWaiters.delete(wake);
          reject(new Error(`deadline: ${inFlight} refetch(es) still in flight after ${deadlineMs}ms`));
        }, deadlineMs);
        idleWaiters.add(wake);
      });
    },
    stop() {
      for (const unlisten of unlisteners) unlisten();
      webEvents.__resetWebEventsForTest();
    },
  };
}
