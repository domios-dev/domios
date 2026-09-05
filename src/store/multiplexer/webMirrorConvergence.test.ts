// Run: npx tsx src/store/multiplexer/webMirrorConvergence.test.ts
// Convergence oracle for the web mirror: given a RECORDED sequence of tab
// switches and event deltas including an overflow, the store must end equal to
// the daemon snapshot. The sequence is data, so nothing here depends on timing.

import { reducer, type Action } from "./reducer";
import type { ExtendedState } from "./tree";
import type { MultiplexerState, Tab, Workspace } from "../../lib/types";

let failures = 0;
function test(name: string, fn: () => void) {
  try {
    fn();
    console.log(`  [ok] ${name}`);
  } catch (e) {
    failures++;
    console.error(`  [x] ${name}: ${e instanceof Error ? e.message : String(e)}`);
  }
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

function base(): ExtendedState {
  return {
    workspaces: [], tabs: {}, teams: [], active_workspace_id: null,
    focused_tab_id: null, notifications: [], tabStatuses: {}, toasts: [],
    activeTabIds: {}, generation: 0, pendingCloses: [],
  };
}

const WS = "ws-1";

/** The daemon's state after `switches` switches: `tabCount` tabs, one focused. */
function daemonSnapshot(generation: number, tabCount: number, focusedIndex: number): MultiplexerState {
  const tabIds = Array.from({ length: tabCount }, (_, i) => `t-${i}`);
  const tabs: Record<string, Tab> = {};
  for (const id of tabIds) {
    tabs[id] = { id, title: `tab ${id}`, agent_type: null } as unknown as Tab;
  }
  const workspace = {
    id: WS, name: "W", tabs: tabIds, active_tab_id: tabIds[focusedIndex] ?? null, cwd: null,
  } as unknown as Workspace;
  return {
    workspaces: [workspace],
    tabs,
    teams: [],
    active_workspace_id: WS,
    focused_tab_id: tabIds[focusedIndex] ?? null,
    generation,
  };
}

/** One recorded step of the mirror's life. */
type Step =
  | { kind: "switch"; focusedIndex: number }
  | { kind: "overflow" }
  | { kind: "peer-switch" };

/**
 * Replays a recorded sequence the way the web mirror consumes it. Every
 * `mux-state-changed` on the wire is a bare signal, so each switch costs a
 * full snapshot refetch; `deliver` decides the order those refetches resolve
 * in, which is what a switch storm actually varies.
 */
function replay(
  steps: Step[],
  deliver: (snapshots: MultiplexerState[]) => MultiplexerState[],
  startGeneration = 0,
): { store: ExtendedState; daemon: MultiplexerState } {
  let generation = startGeneration;
  let store = base();
  const dispatch = (action: Action) => { store = reducer(store, action); };
  const pending: MultiplexerState[] = [];
  let latest = daemonSnapshot(generation, 8, 0);

  for (const step of steps) {
    if (step.kind === "peer-switch") {
      dispatch({ type: "RESET_PEER_CONTEXT" });
      continue;
    }
    if (step.kind === "overflow") {
      // runFullResync: refetch reachable state. Same refetch, no extra ordering.
      pending.push(latest);
      continue;
    }
    generation += 1;
    latest = daemonSnapshot(generation, 8, step.focusedIndex);
    pending.push(latest);
  }
  for (const snapshot of deliver(pending)) {
    dispatch({ type: "SET_STATE", state: snapshot });
  }
  return { store, daemon: latest };
}

function assertConverged(store: ExtendedState, daemon: MultiplexerState, label: string): void {
  assert(
    Object.keys(store.tabs).sort().join(",") === Object.keys(daemon.tabs).sort().join(","),
    `${label}: tab set differs (store=${Object.keys(store.tabs).length} daemon=${Object.keys(daemon.tabs).length})`,
  );
  assert(
    store.focused_tab_id === daemon.focused_tab_id,
    `${label}: focus differs (store=${store.focused_tab_id} daemon=${daemon.focused_tab_id})`,
  );
  assert(
    store.generation === daemon.generation,
    `${label}: generation differs (store=${store.generation} daemon=${daemon.generation})`,
  );
}

const inOrder = (s: MultiplexerState[]): MultiplexerState[] => s;
const reversed = (s: MultiplexerState[]): MultiplexerState[] => [...s].reverse();
/** Deterministic interleave standing in for overlapping refetches resolving
 *  out of order - the shape a relayed round trip produces. */
const shuffledPairs = (s: MultiplexerState[]): MultiplexerState[] => {
  const out: MultiplexerState[] = [];
  for (let i = 0; i < s.length; i += 2) {
    if (i + 1 < s.length) out.push(s[i + 1], s[i]);
    else out.push(s[i]);
  }
  return out;
};

const STORM: Step[] = [
  ...Array.from({ length: 12 }, (_, i) => ({ kind: "switch" as const, focusedIndex: i % 8 })),
  { kind: "overflow" as const },
  ...Array.from({ length: 12 }, (_, i) => ({ kind: "switch" as const, focusedIndex: (i + 3) % 8 })),
  { kind: "overflow" as const },
  ...Array.from({ length: 6 }, (_, i) => ({ kind: "switch" as const, focusedIndex: (i + 1) % 8 })),
];

console.log("web mirror convergence");

for (const [name, deliver] of [
  ["in order", inOrder],
  ["reversed", reversed],
  ["overlapping refetches", shuffledPairs],
] as const) {
  test(`switch storm with overflow converges - ${name} delivery`, () => {
    const { store, daemon } = replay(STORM, deliver);
    assertConverged(store, daemon, name);
  });
}

test("peer-sourced events converge after a peer switch", () => {
  // A peer's generation counter is its own and may start far below the origin
  // the store was last showing. RESET_PEER_CONTEXT is what makes the peer's
  // first snapshot adoptable; convergence must then hold as it does locally.
  let store = base();
  store = reducer(store, { type: "SET_STATE", state: daemonSnapshot(9_000, 8, 2) });
  assert(store.generation === 9_000, "precondition: local origin snapshot adopted");

  store = reducer(store, { type: "RESET_PEER_CONTEXT" });
  assert(store.generation === -1, "peer switch must clear the origin's generation");
  assert(Object.keys(store.tabs).length === 0, "peer switch must clear the origin's tabs");

  const { store: peerStore, daemon } = replay(STORM, shuffledPairs, 0);
  assertConverged(peerStore, daemon, "peer-sourced");
  assert(
    (daemon.generation ?? Number.MAX_SAFE_INTEGER) < 9_000,
    "fixture check: the peer's counter is below the previous origin's",
  );
});

test("a peer snapshot below the previous origin's generation is still adopted", () => {
  let store = base();
  store = reducer(store, { type: "SET_STATE", state: daemonSnapshot(9_000, 8, 2) });
  store = reducer(store, { type: "RESET_PEER_CONTEXT" });
  const peerFirst = daemonSnapshot(17, 8, 5);
  store = reducer(store, { type: "SET_STATE", state: peerFirst });
  assertConverged(store, peerFirst, "peer first snapshot");
});

console.log(failures === 0 ? "\nall passed" : `\n${failures} failed`);
if (failures > 0) throw new Error(`${failures} test(s) failed`);
