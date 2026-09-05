/**
 * The reproduction: many tabs, a switch storm, an induced overflow, and one
 * assertion - the web store equals the daemon.
 *
 * Pacing rule: a switch is issued and then *waited for* on the daemon's own
 * `tab-focused` frame before the next is issued. The switch rate is therefore
 * an observation reported in the run row, never a delay this file chooses.
 * The only timers are failure deadlines.
 */
import { startMirror, assertDevOrigin, type Mirror } from "./mirror";

const FRAME_DEADLINE_MS = 30_000;
/** Catch-up is a per-batch check, not a stream wait - a short bound keeps a
 *  storm of lagging batches from turning into a stalled run. */
const CATCHUP_DEADLINE_MS = 8_000;
/** createRefreshCoalescer bounds concurrent full-state fetches to one. Before
 *  it, this bench measured 1028-1050 and the daemon died under that load. */
const IN_FLIGHT_CEILING = 1;
/** api/events.rs RING_MAX_COUNT. Exceeding it while parked forces `ring-evicted`. */
const RING_MAX_COUNT = 2048;
/** A focus emits `mux-state-changed` + `tab-focused`. */
const EVENTS_PER_SWITCH = 2;

export interface ScenarioOptions {
  origin: string;
  token: string;
  workspaceId: string;
  tabIds: string[];
  switches: number;
  /** Switches in flight at once. A real user outruns the round trip; an
   *  ack-paced storm cannot produce overlapping snapshot refetches. */
  concurrency: number;
  /** Skip the park/flood/overflow leg. Leaves a pure connected-throughput run
   *  whose refetch count is directly comparable to its switch count - the
   *  measurement behind "one full state refetch per event". */
  skipOverflow: boolean;
  /** Origin switches (local <-> peer) to perform after the storm. Tests P1/P2
   *  in FINDINGS 4c. Requires a relay peer. */
  originSwitches: number;
  relayPeer?: { id: string; name: string } | null;
}

export interface RunRow {
  origin: "local" | "peer";
  tabs: number;
  concurrency: number;
  switches: number;
  switch_rate_hz: number;
  overflow_fired: number;
  overflow_reason: string;
  elapsed_ms: number;
  resync_fanout: number;
  mux_refetches: number;
  refetch_errors: number;
  max_in_flight: number;
  refetch_max_ms: number;
  client_gen: number;
  daemon_gen: number;
  diverged: boolean;
  divergence_kind: string;
}

export interface DaemonSnapshot {
  generation: number;
  focused_tab_id: string | null;
  tabs: Record<string, { title?: string }>;
}

async function api<T>(o: ScenarioOptions, method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`${o.origin}${path}`, {
    method,
    headers: { Authorization: `Bearer ${o.token}`, "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`${method} ${path} -> HTTP ${res.status}`);
  const text = await res.text();
  return (text ? JSON.parse(text) : null) as T;
}

const focusPath = (o: ScenarioOptions, tabId: string): string =>
  `/api/workspaces/${o.workspaceId}/tabs/${tabId}/focus`;

/**
 * Batch acknowledgement. Under overlapping switches the daemon coalesces and
 * does NOT emit one `tab-focused` per request, so a per-switch frame is not a
 * valid ack. The valid ack is convergence itself: read the generation the
 * daemon had already reached once every request in the batch had answered,
 * then wait for the client's store to reach it. Event-driven; the deadline is
 * a failure bound, and hitting it IS the divergence being hunted.
 */
async function catchUp(mirror: Mirror, o: ScenarioOptions): Promise<boolean> {
  const daemon = await api<DaemonSnapshot>(o, "GET", "/api/mux/state");
  if (mirror.state().generation >= daemon.generation) return true;
  try {
    await mirror.awaitFrame(
      () => mirror.state().generation >= daemon.generation,
      CATCHUP_DEADLINE_MS,
      `store generation >= ${daemon.generation}`,
    );
    return true;
  } catch {
    return false;
  }
}

/**
 * Deterministic overflow. Park the event stream (a dropped SSE path - what a
 * mesh relay hiccup or a backgrounded browser actually does), keep switching
 * so the daemon's ring advances past what it retains, then release. The
 * client reconnects on a cursor older than `oldest`, which `RingSnapshot::plan`
 * must classify `Evicted` and answer with `ring-evicted`. The trigger is a
 * count, not a duration.
 */
async function induceOverflow(mirror: Mirror, o: ScenarioOptions): Promise<number> {
  const before = mirror.overflows();
  mirror.gate.park();
  const floodSwitches = Math.ceil((RING_MAX_COUNT * 1.3) / EVENTS_PER_SWITCH);
  for (let i = 0; i < floodSwitches; i++) {
    await api(o, "POST", focusPath(o, o.tabIds[i % o.tabIds.length]));
  }
  const overflowed = mirror.awaitFrame(
    () => mirror.overflows() > before,
    FRAME_DEADLINE_MS,
    "overflow after ring eviction",
  );
  mirror.gate.release();
  await overflowed;
  return floodSwitches;
}

function compare(mirror: Mirror, daemon: DaemonSnapshot): { diverged: boolean; kind: string } {
  const client = mirror.state();
  const clientIds = new Set(Object.keys(client.tabs));
  const daemonIds = new Set(Object.keys(daemon.tabs));
  const missing = [...daemonIds].filter((id) => !clientIds.has(id));
  const extra = [...clientIds].filter((id) => !daemonIds.has(id));
  if (missing.length > 0) return { diverged: true, kind: `missing:${missing.length}` };
  if (extra.length > 0) return { diverged: true, kind: `extra:${extra.length}` };
  if (client.focused_tab_id !== daemon.focused_tab_id) return { diverged: true, kind: "focus" };
  const titleDrift = [...daemonIds].filter((id) => client.tabs[id]?.title !== daemon.tabs[id]?.title);
  if (titleDrift.length > 0) return { diverged: true, kind: `title:${titleDrift.length}` };
  return { diverged: false, kind: "" };
}

export async function runScenario(o: ScenarioOptions): Promise<RunRow> {
  assertDevOrigin(o.origin);
  const mirror = await startMirror({ origin: o.origin, token: o.token, relayPeer: o.relayPeer ?? null });
  try {
    await mirror.awaitStreamOpen(FRAME_DEADLINE_MS);
    await api(o, "POST", focusPath(o, o.tabIds[0]));
    await catchUp(mirror, o);

    let laggedBatches = 0;
    const started = performance.now();
    for (let i = 0; i < o.switches; i += o.concurrency) {
      const batch: Array<Promise<unknown>> = [];
      for (let k = 0; k < o.concurrency && i + k < o.switches; k++) {
        batch.push(api(o, "POST", focusPath(o, o.tabIds[(i + k) % o.tabIds.length])));
      }
      await Promise.all(batch);
      if (!(await catchUp(mirror, o))) laggedBatches++;
    }
    // Origin switches: tests P1 (one boot-changed overflow per switch) and
    // P2 (output stream does not follow the origin). FINDINGS 4c.
    let originSwitchOverflows = 0;
    if (o.originSwitches > 0 && o.relayPeer) {
      const before = mirror.overflows();
      for (let i = 0; i < o.originSwitches; i++) {
        const opens = mirror.streamOpens();
        mirror.switchOrigin(i % 2 === 0 ? null : o.relayPeer);
        await mirror.awaitStreamOpen(FRAME_DEADLINE_MS, opens + 1);
        await catchUp(mirror, o);
      }
      originSwitchOverflows = mirror.overflows() - before;
    }

    let flood = 0;
    if (!o.skipOverflow) {
      flood = await induceOverflow(mirror, o);
      // The overflow's own refetch must land before the oracle reads the store.
      const refetches = mirror.counters.muxRefetches;
      await mirror.awaitFrame(
        () => mirror.counters.muxRefetches > refetches,
        FRAME_DEADLINE_MS,
        "post-overflow refetch",
      );
    }
    const elapsed = performance.now() - started;
    // Quiesce: the last snapshot the mirror asked for must have been applied.
    await mirror.settle(FRAME_DEADLINE_MS);

    const daemon = await api<DaemonSnapshot>(o, "GET", "/api/mux/state");
    const verdict = laggedBatches > 0
      ? { diverged: true, kind: `generation-lag:${laggedBatches}` }
      : compare(mirror, daemon);
    const totalSwitches = o.switches + flood;
    return {
      origin: o.relayPeer ? "peer" : "local",
      tabs: o.tabIds.length,
      concurrency: o.concurrency,
      switches: totalSwitches,
      switch_rate_hz: Number(((totalSwitches / elapsed) * 1000).toFixed(2)),
      overflow_fired: mirror.overflows(),
      overflow_reason: mirror.bootChanges() > 0 ? "boot-changed" : "ring-evicted",
      origin_switches: o.originSwitches,
      overflow_per_origin_switch: o.originSwitches > 0
        ? Number((originSwitchOverflows / o.originSwitches).toFixed(2))
        : 0,
      elapsed_ms: Math.round(elapsed),
      resync_fanout: mirror.counters.resyncFanout,
      mux_refetches: mirror.counters.muxRefetches,
      refetch_errors: mirror.counters.refetchErrors,
      max_in_flight: mirror.counters.maxInFlight,
      refetch_max_ms: mirror.counters.refetchMaxMs,
      client_gen: mirror.state().generation,
      daemon_gen: daemon.generation,
      diverged: verdict.diverged,
      divergence_kind: verdict.kind,
    };
  } finally {
    mirror.stop();
  }
}
