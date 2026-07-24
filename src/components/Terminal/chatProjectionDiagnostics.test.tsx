// Run: node node_modules/tsx/dist/cli.cjs src/components/Terminal/chatProjectionDiagnostics.test.tsx
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import {
  act,
  createElement,
  useDeferredValue,
  useEffect,
  useState,
} from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { ChatProjectionMonitor, scheduleAfterPaint } from "./chatProjectionDiagnostics";
import { paintTailSignature, tailSignatureOf } from "./useChatSession";
import type { ChatSessionDiagnosticEvent } from "./useChatSession";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

// A callback-driven counter read through a function boundary. Asserting a live
// `let` counter with `=== 0` then later `=== 1` makes control-flow narrow it to
// the literal `0`, so the second comparison is a spurious TS2367; a getter is not
// narrowed by the assert.
function hitCounter() {
  let n = 0;
  return { hit: (): void => { n += 1; }, count: (): number => n };
}

const PANE = "pane";
// Default run documents the CURRENT (starved) contract and is green. Setting
// CHAT_PROJECTION_ASSERT_FIXED=1 flips the streaming assertions to the
// post-correction contract, which fails at the current head — the reproducible
// fails-before baseline a later rendering fix is expected to turn green.
const ASSERT_FIXED = process.env.CHAT_PROJECTION_ASSERT_FIXED === "1";

type EvMsg = { id: string; content?: string };
function arrival(paneId: string, msgs: EvMsg[]): ChatSessionDiagnosticEvent {
  return { edge: "event_arrival", paneId, sourceSessionId: null, sessionBindingNonce: null, messageIds: msgs.map((m) => m.id), tailSignature: tailSignatureOf(msgs) };
}
function adopt(paneId: string, msgs: EvMsg[]): ChatSessionDiagnosticEvent {
  return { edge: "authority_adopt", paneId, sourceSessionId: null, sessionBindingNonce: null, messageIds: msgs.map((m) => m.id), tailSignature: tailSignatureOf(msgs) };
}
function drop(paneId: string, msgs: EvMsg[]): ChatSessionDiagnosticEvent {
  return { edge: "authority_drop", paneId, sourceSessionId: null, sessionBindingNonce: null, messageIds: msgs.map((m) => m.id), tailSignature: tailSignatureOf(msgs) };
}
function commit(paneId: string, msgs: EvMsg[]): ChatSessionDiagnosticEvent {
  return { edge: "state_commit", paneId, sourceSessionId: null, sessionBindingNonce: null, messageIds: msgs.map((m) => m.id), tailSignature: tailSignatureOf(msgs) };
}
function snapshotArrival(
  paneId: string,
  msgs: EvMsg[],
  coords: { cursor?: string | null; offset?: number | null; startOffset?: number | null } = {},
): ChatSessionDiagnosticEvent {
  return {
    edge: "snapshot_arrival",
    paneId,
    sourceSessionId: null,
    sessionBindingNonce: null,
    messageIds: msgs.map((m) => m.id),
    subscriptionId: null,
    tailSignature: tailSignatureOf(msgs),
    cursor: coords.cursor ?? null,
    offset: coords.offset ?? null,
    startOffset: coords.startOffset ?? null,
  };
}
// Drive a pane up to a committed+painted steady state at the given tail.
function bringToSteadyState(monitor: ChatProjectionMonitor, paneId: string, msgs: EvMsg[]): void {
  monitor.ingest(snapshotArrival(paneId, msgs, { cursor: "c0", offset: msgs.length, startOffset: 0 }));
  monitor.ingest(arrival(paneId, msgs));
  monitor.ingest(adopt(paneId, msgs));
  monitor.ingest(commit(paneId, msgs));
  monitor.recordPaint(paneId, tailSignatureOf(msgs));
}

// ── Unit: distinct per-stage counters ──────────────────────────────────────
function testStageDistinction(): void {
  const monitor = new ChatProjectionMonitor();
  const list: EvMsg[] = [{ id: "m0", content: "aa" }, { id: "m1", content: "bbb" }];
  const tail = tailSignatureOf(list); // "m1#3"
  monitor.ingest(arrival(PANE, list));
  monitor.ingest(adopt(PANE, list));
  monitor.ingest(commit(PANE, list));
  monitor.recordPaint(PANE, tail);

  const snap = monitor.snapshot(PANE)!;
  assert(snap.producedMessages === 2, "producer counter sums delivered messages");
  assert(snap.arrived === 1, "backend-emit counter counts delivered delta events");
  assert(snap.adopted === 1 && snap.dropped === 0, "authority distinguishes adopt from drop");
  assert(snap.committed === 1, "committed-state counter distinct from arrival");
  assert(snap.painted === 1, "painted-tail counter distinct from committed state");
  assert(snap.arrivalTail === tail && snap.committedTail === tail && snap.paintedTail === tail, "tails track the newest content signature");
  assert(monitor.firstStalledEdge(PANE) === null, "fully caught-up chain reports no stall");
}

// ── Unit: authority drop surfaces as an arrival→authority gap ───────────────
function testAuthorityStall(): void {
  const monitor = new ChatProjectionMonitor();
  const own: EvMsg[] = [{ id: "own", content: "hi" }];
  monitor.ingest(arrival(PANE, own));
  monitor.ingest(adopt(PANE, own));
  monitor.ingest(commit(PANE, own));
  monitor.recordPaint(PANE, tailSignatureOf(own));
  // A foreign-session delta arrives and is rejected: arrival tail advances, authority does not.
  const foreign: EvMsg[] = [{ id: "foreign", content: "yo" }];
  monitor.ingest(arrival(PANE, foreign));
  monitor.ingest(drop(PANE, foreign));

  const snap = monitor.snapshot(PANE)!;
  assert(snap.dropped === 1 && snap.arrivalTail === "foreign#2", "rejected delta advances arrival, not authority");
  assert(snap.authorityTail === "own#2", "authority tail holds at the last adopted signature");
  const edge = monitor.firstStalledEdge(PANE);
  assert(edge?.from === "backend_emit" && edge?.to === "frontend_authority", "authority rejection is the first stalled edge");
}

// ── Unit: monotonic + bounded ───────────────────────────────────────────────
function testMonotonicAndBounded(): void {
  const monitor = new ChatProjectionMonitor(2);
  for (let i = 0; i < 5; i += 1) monitor.ingest(arrival(PANE, [{ id: `m${i}`, content: "x" }]));
  assert(monitor.snapshot(PANE)!.arrived === 5, "arrival counter only increases");

  monitor.ingest(arrival("a", [{ id: "x", content: "x" }]));
  monitor.ingest(arrival("b", [{ id: "y", content: "y" }]));
  assert(monitor.trackedPaneCount() === 2, "tracked panes capped at bound");
  monitor.ingest(arrival(PANE, [{ id: "z", content: "z" }])); // touch pane -> evicts the LRU pane ("a")
  assert(monitor.trackedPaneCount() === 2, "adding beyond the cap evicts, stays bounded");
  assert(monitor.snapshot("a") === null, "least-recently-used pane evicted");
  assert(monitor.snapshot(PANE) !== null && monitor.snapshot("b") !== null, "recently-used panes retained");
}

// ── Unit: lifecycle cleanup — records do not outlive their pane ──────────────
function testForgetLifecycle(): void {
  const monitor = new ChatProjectionMonitor();
  monitor.ingest(arrival(PANE, [{ id: "m0", content: "hi" }]));
  assert(monitor.trackedPaneCount() === 1, "pane tracked after ingest");
  monitor.forget(PANE);
  assert(monitor.snapshot(PANE) === null, "forget drops the pane record");
  assert(monitor.trackedPaneCount() === 0, "forget shrinks the tracked pane count, not just evicts on cap");
}

// ── Unit: painted-tail probe fires after paint (double rAF), not at commit ───
function testScheduleAfterPaint(): void {
  const g = globalThis as unknown as {
    requestAnimationFrame?: (cb: () => void) => number;
    cancelAnimationFrame?: (id: number) => void;
  };
  const prevRaf = g.requestAnimationFrame;
  const prevCaf = g.cancelAnimationFrame;
  const pending = new Map<number, () => void>();
  let nextId = 1;
  g.requestAnimationFrame = (cb) => {
    const id = nextId++;
    pending.set(id, cb);
    return id;
  };
  g.cancelAnimationFrame = (id) => {
    pending.delete(id);
  };
  const runOne = (): boolean => {
    const first = pending.entries().next().value as [number, () => void] | undefined;
    if (!first) return false;
    pending.delete(first[0]);
    first[1]();
    return true;
  };
  try {
    // Happy path: one frame is React commit, the second is post-paint.
    const painted = hitCounter();
    scheduleAfterPaint(painted.hit);
    assert(painted.count() === 0, "no record before any frame");
    runOne();
    assert(painted.count() === 0, "no record after a single frame (React commit, not paint)");
    runOne();
    assert(painted.count() === 1, "record fires after the second frame (post-paint)");

    // Cancel before the first frame: nothing pending, nothing recorded.
    const early = hitCounter();
    scheduleAfterPaint(early.hit)();
    while (runOne()) { /* drain */ }
    assert(early.count() === 0, "cancelling before the first frame suppresses the record");

    // Cancel after the outer frame (renders outrun paints): inner frame cancelled.
    const superseded = hitCounter();
    const cancel = scheduleAfterPaint(superseded.hit);
    runOne(); // outer runs, schedules inner
    cancel(); // cancel the pending inner frame
    while (runOne()) { /* drain */ }
    assert(superseded.count() === 0, "cancelling a superseded frame before paint suppresses its stale record");
  } finally {
    g.requestAnimationFrame = prevRaf;
    g.cancelAnimationFrame = prevCaf;
  }

  // No rAF host (non-browser/test): records synchronously so the probe still works.
  const prevRaf2 = g.requestAnimationFrame;
  g.requestAnimationFrame = undefined;
  try {
    const sync = hitCounter();
    scheduleAfterPaint(sync.hit);
    assert(sync.count() === 1, "falls back to a synchronous record when no rAF exists");
  } finally {
    g.requestAnimationFrame = prevRaf2;
  }
}

// ── Unit: frozen tail under STABLE-ID streaming (the real streaming shape) ────
function testFrozenTailStableIdStreaming(): void {
  const monitor = new ChatProjectionMonitor();
  // Baseline caught up: a prior assistant turn "a1" and the streaming turn seed.
  const base: EvMsg[] = [{ id: "m0", content: "hello" }, { id: "a1", content: "start" }];
  monitor.ingest(arrival(PANE, base));
  monitor.ingest(adopt(PANE, base));
  monitor.ingest(commit(PANE, base));
  monitor.recordPaint(PANE, tailSignatureOf(base)); // "a1#5"
  assert(monitor.firstStalledEdge(PANE) === null, "baseline steady state has no stalled edge");

  // Streaming grows the SAME message id "a1"; paint is starved during urgent load.
  // With an id-only tail this would be a false negative — arrival/commit/paint all
  // read tail id "a1" and the stall detector would see them agree. The content
  // signature is what keeps the committed-but-unpainted growth visible.
  const streamCount = 48;
  let content = "start";
  for (let i = 0; i < streamCount; i += 1) {
    content += "x";
    const delta: EvMsg[] = [{ id: "a1", content }];
    monitor.ingest(arrival(PANE, delta));
    monitor.ingest(adopt(PANE, delta));
    monitor.ingest(commit(PANE, delta));
    // No recordPaint — the downstream projection never runs during urgent load.
    const edge = monitor.firstStalledEdge(PANE);
    assert(
      edge?.from === "committed_state" && edge?.to === "painted_tail",
      `stable-id frozen tail: committed grew to ${content.length} chars but paint stuck at ${edge?.observedTail}`,
    );
  }

  const loaded = monitor.snapshot(PANE)!;
  const finalTail = `a1#${5 + streamCount}`; // "a1#53"
  assert(loaded.committed === streamCount + 1, "every streamed delta committed state");
  assert(loaded.painted === 1, "paint stayed frozen through the whole urgent load");
  assert(loaded.committedTail === finalTail && loaded.paintedTail === "a1#5", "committed content signature advanced while painted signature froze under the stable id");

  // Release: one collapsed paint catches the visible tail up to the committed tail.
  monitor.recordPaint(PANE, finalTail);
  assert(monitor.firstStalledEdge(PANE) === null, "a single collapsed paint clears the stall (not mistaken for lag)");
  assert(monitor.snapshot(PANE)!.painted === 2, "collapsed catch-up is one paint, not per-delta");
}

// ── Unit: snapshot_arrival records cursor/offset/signature (recovery coords) ──
function testSnapshotSignatureRecorded(): void {
  const monitor = new ChatProjectionMonitor();
  const msgs: EvMsg[] = [{ id: "m0", content: "hi" }, { id: "m1", content: "there" }];
  monitor.ingest(snapshotArrival(PANE, msgs, { cursor: "cur-7", offset: 2, startOffset: 0 }));
  const snap = monitor.snapshot(PANE)!;
  assert(snap.snapshotTail === "m1#5", "snapshot records the transcript tail signature it recovered to");
  assert(snap.snapshotCursor === "cur-7", "snapshot records the backend cursor");
  assert(snap.snapshotOffset === 2, "snapshot records the backend offset");
  // Recovery coordinates are metadata, not a projection stage: they never invent a stall.
  assert(monitor.firstStalledEdge(PANE) === null, "snapshot coordinates do not participate in firstStalledEdge");
}

// ── Unit: away→back / Terminal→Chat activation reconcile oracle ───────────────
// Terminal↔Chat toggling unmounts/remounts PaneSessionChat, so forget() runs on
// the way out and a fresh snapshot on the way back. The oracle proves the prior
// committed tail survives teardown and the reactivated snapshot is attributed by
// its cursor/signature — a clean recovery, a same-id regression, or a divergence.
function testActivationReconcileOracle(): void {
  // Clean recovery: the re-attach snapshot returns the full prior transcript.
  {
    const monitor = new ChatProjectionMonitor();
    const loaded: EvMsg[] = [{ id: "m0", content: "hello" }, { id: "a1", content: "start-xxxxx" }];
    bringToSteadyState(monitor, PANE, loaded); // committed/painted tail "a1#11"
    assert(monitor.activationRecovery(PANE) === "no_prior", "a live pane never torn down has no prior activation");

    monitor.forget(PANE); // Terminal switch: unmount
    assert(monitor.snapshot(PANE) === null, "forget drops the live record");
    assert(monitor.trackedPaneCount() === 0, "torn-down pane is not counted as live");
    assert(monitor.firstStalledEdge(PANE) === null, "a forgotten pane reports no live stall");
    const prior = monitor.priorActivation(PANE)!;
    assert(prior.committedTail === "a1#11", "prior committed tail is preserved across teardown");
    assert(prior.snapshotCursor === "c0", "prior recovery cursor is preserved across teardown");
    assert(monitor.activationRecovery(PANE) === "pending", "torn down but not yet reactivated is pending, not a false desync");

    // Back to Chat: a fresh snapshot recovers to the same committed tail.
    monitor.ingest(snapshotArrival(PANE, loaded, { cursor: "c1", offset: 2 }));
    assert(monitor.snapshot(PANE)!.snapshotTail === "a1#11", "reactivated snapshot tail recorded on the fresh live record");
    assert(monitor.activationRecovery(PANE) === "recovered", "away→back with a full re-attach is a clean signature-gated recovery");
  }

  // Regression: the re-attach lands on the same tail id but shorter content — the
  // backend tailer came back with less than the pane had already committed.
  {
    const monitor = new ChatProjectionMonitor();
    bringToSteadyState(monitor, PANE, [{ id: "m0", content: "hi" }, { id: "a1", content: "abcdefghij" }]); // "a1#10"
    monitor.forget(PANE);
    monitor.ingest(snapshotArrival(PANE, [{ id: "m0", content: "hi" }, { id: "a1", content: "abc" }], { cursor: "c1" })); // "a1#3"
    assert(monitor.activationRecovery(PANE) === "regressed", "a shorter same-id re-attach is flagged as a regression, not a clean recovery");
  }

  // Divergence: the re-attach drops the streamed tail message entirely (lands on a
  // different id) — unorderable by signature alone; the archived cursor/offset stay
  // available for deeper attribution.
  {
    const monitor = new ChatProjectionMonitor();
    bringToSteadyState(monitor, PANE, [{ id: "m0", content: "hi" }, { id: "a1", content: "streamed" }]); // "a1#8"
    monitor.forget(PANE);
    monitor.ingest(snapshotArrival(PANE, [{ id: "m0", content: "hi" }], { cursor: "c1", offset: 1 })); // "m0#2"
    assert(monitor.activationRecovery(PANE) === "diverged", "a re-attach on a different tail id is flagged as diverged");
    assert(monitor.priorActivation(PANE)!.committedTail === "a1#8", "archived prior tail remains readable for cursor/offset attribution");
  }
}

// ── Unit: test-only exposure of the live monitor for E2E/integration probes ───
function testTestOnlyExposure(): void {
  // Mirrors PaneSessionChat's test-mode wiring: expose the process-wide monitor on
  // window under a test-only key so harnesses can read live projection state.
  const monitor = new ChatProjectionMonitor();
  const w = globalThis as unknown as { __chatProjection?: ChatProjectionMonitor };
  const prev = w.__chatProjection;
  try {
    w.__chatProjection ??= monitor;
    assert(w.__chatProjection === monitor, "test-only global exposes the live monitor instance");
    monitor.ingest(arrival(PANE, [{ id: "m0", content: "x" }]));
    assert(w.__chatProjection.snapshot(PANE)!.arrived === 1, "read seams are reachable through the test-only global");
    // Idempotent: the ??= exposure does not clobber an already-published monitor.
    const other = new ChatProjectionMonitor();
    w.__chatProjection ??= other;
    assert(w.__chatProjection === monitor, "exposure is idempotent — an existing monitor is not overwritten");
  } finally {
    w.__chatProjection = prev;
  }
}

// ── Integration: real useChatSession feeds the monitor; frozen tail emerges ──
function setupHookDom(): void {
  const hookWindow = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, value: true, writable: true });
  Object.defineProperty(globalThis, "window", { configurable: true, value: hookWindow });
  Object.defineProperty(globalThis, "document", { configurable: true, value: hookWindow.document });
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: hookWindow.navigator });
  Object.defineProperty(globalThis, "HTMLElement", { configurable: true, value: hookWindow.HTMLElement });
  Object.defineProperty(globalThis, "Event", { configurable: true, value: hookWindow.Event });
  Object.defineProperty(globalThis, "CustomEvent", { configurable: true, value: hookWindow.CustomEvent });
}

function msg(id: string, content = id): NormalizedChatMessage {
  return { id, type: "assistant", content, timestamp: id.slice(1) || "0" };
}

async function flush(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
}

type ChatHandler = (event: { payload: unknown }) => void;

async function testFrozenTailSignatureIntegration(): Promise<void> {
  setupHookDom();
  const bridge = await import("../../lib/bridge");
  const { useChatSession } = await import("./useChatSession");

  const snapshot: NormalizedChatMessage[] = [msg("m0")];
  let handler: ChatHandler | null = null;
  (bridge as never as { __setListenForTest: (fn: unknown) => void }).__setListenForTest(
    async (_event: string, next: ChatHandler) => {
      handler = next;
      return () => { handler = null; };
    },
  );
  (bridge as never as { __setInvokeForTest: (fn: unknown) => void }).__setInvokeForTest(
    async (command: string, args: Record<string, unknown>) => {
      if (command === "chat_start") {
        return {
          text: "", start_offset: 0, offset: snapshot.length, pending: false,
          messages: [...snapshot], source_session_id: "session-own",
          session_binding_nonce: "nonce-a", subscription_id: String(args.subscriptionId ?? ""),
        };
      }
      return {};
    },
  );

  const monitor = new ChatProjectionMonitor();
  let bumpUrgent: (() => void) | null = null;

  function Harness() {
    const [urgent, setUrgent] = useState(0);
    bumpUrgent = () => setUrgent((v) => v + 1);
    const session = useChatSession({
      paneId: PANE,
      agentSessionId: "session-own",
      sessionBindingNonce: "nonce-a",
      active: true,
      diagnosticSink: monitor.sink,
    });
    const deferred = useDeferredValue(session.enrichedMessages);
    // Mirror production: passive, post-paint probe recording the content signature.
    useEffect(() => {
      const tail = paintTailSignature(deferred[deferred.length - 1]?.msg);
      return scheduleAfterPaint(() => monitor.recordPaint(PANE, tail));
    }, [deferred]);
    return createElement("output", { "data-urgent": urgent }, String(deferred.length));
  }

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => { root.render(createElement(Harness)); });
  await flush();

  assert(monitor.firstStalledEdge(PANE) === null, "integration: attach settles with no stalled edge");
  const baseCommitted = monitor.snapshot(PANE)!.committed;
  const basePaintedTail = monitor.snapshot(PANE)!.paintedTail; // "m0#2"

  // Streaming appends ONE assistant message "m1" and grows its content under a
  // stable id — the real streaming shape the id-only probe used to miss.
  const streamCount = 48;
  let committedDuringLoad = -1;
  let committedTailDuringLoad: string | null = null;
  let paintedTailDuringLoad: string | null = null;
  await act(async () => {
    for (let i = 0; i < streamCount; i += 1) {
      handler?.({
        payload: {
          pane_id: PANE, messages: [msg("m1", "x".repeat(i + 1))],
          source_session_id: "session-own", session_binding_nonce: "nonce-a",
        },
      });
      flushSync(bumpUrgent!);
      const edge = monitor.firstStalledEdge(PANE);
      if (ASSERT_FIXED) {
        // The post-correction contract: the visible tail keeps up with committed
        // state during streaming. This FAILS at the current (starved) head — the
        // reproducible red baseline that a later rendering fix must turn green.
        assert(
          edge === null,
          `corrected contract after delta ${i + 1}: visible tail should keep up, but stalled ${edge?.from}->${edge?.to}`,
        );
      } else {
        assert(
          edge?.from === "committed_state" && edge?.to === "painted_tail",
          `integration frozen tail after delta ${i + 1}: stalled edge was ${edge?.from}->${edge?.to}`,
        );
      }
    }
    // Capture INSIDE the act, before its final flush lets the deferred projection catch up.
    const snap = monitor.snapshot(PANE)!;
    committedDuringLoad = snap.committed;
    committedTailDuringLoad = snap.committedTail;
    paintedTailDuringLoad = snap.paintedTail;
  });

  assert(committedDuringLoad - baseCommitted === streamCount, "integration: urgent ingestion committed every delta");
  assert(committedTailDuringLoad === `m1#${streamCount}`, "integration: committed content signature reached the newest delta");
  if (ASSERT_FIXED) {
    assert(paintedTailDuringLoad === committedTailDuringLoad, "corrected contract: painted tail stayed level with committed tail during load");
  } else {
    // Frozen-tail signature: the visible projection never caught the committed tail
    // during urgent load (the per-delta edge checks proved the lag held throughout).
    assert(paintedTailDuringLoad === basePaintedTail, "integration: painted signature stayed frozen at the pre-load tail through urgent load");
    assert(paintedTailDuringLoad !== committedTailDuringLoad, "integration: painted tail lagged the committed tail through urgent load");
  }

  // Release urgent load: the deferred projection catches up in one collapsed paint.
  await flush();
  assert(monitor.firstStalledEdge(PANE) === null, "integration: paint catches up and clears the stall after load");
  assert(monitor.snapshot(PANE)!.paintedTail === `m1#${streamCount}`, "integration: painted tail catches up to committed tail");

  await act(async () => root.unmount());
  await flush();
  (bridge as never as { __setInvokeForTest: (fn: unknown) => void }).__setInvokeForTest(null as never);
  (bridge as never as { __setListenForTest: (fn: unknown) => void }).__setListenForTest(null as never);
}

// ── Integration: real away→back through useChatSession's snapshot edge ─────────
// Drives the full Terminal→Chat lifecycle: mount → stream → unmount (forget
// archives the prior committed tail) → remount (chat_start re-snapshots). Proves
// the archived tail survives real teardown and the reactivated snapshot is
// attributed as a clean, cursor/signature-gated recovery — end to end, not just
// through the monitor's own API.
async function testActivationRemountIntegration(): Promise<void> {
  setupHookDom();
  const bridge = await import("../../lib/bridge");
  const { useChatSession } = await import("./useChatSession");

  // The backend transcript the tailer would replay on re-attach. It grows as the
  // stream commits, so a remount recovers the full content (clean recovery).
  const backend: NormalizedChatMessage[] = [msg("m0")];
  let cursor = "cur-0";
  let handler: ChatHandler | null = null;
  (bridge as never as { __setListenForTest: (fn: unknown) => void }).__setListenForTest(
    async (_event: string, next: ChatHandler) => {
      handler = next;
      return () => { handler = null; };
    },
  );
  (bridge as never as { __setInvokeForTest: (fn: unknown) => void }).__setInvokeForTest(
    async (command: string, args: Record<string, unknown>) => {
      if (command === "chat_start") {
        return {
          text: "", start_offset: 0, offset: backend.length, cursor, pending: false,
          messages: [...backend], source_session_id: "session-own",
          session_binding_nonce: "nonce-a", subscription_id: String(args.subscriptionId ?? ""),
        };
      }
      return {};
    },
  );

  const monitor = new ChatProjectionMonitor();
  function Harness() {
    const session = useChatSession({
      paneId: PANE,
      agentSessionId: "session-own",
      sessionBindingNonce: "nonce-a",
      active: true,
      diagnosticSink: monitor.sink,
    });
    const deferred = useDeferredValue(session.enrichedMessages);
    // Mirror production: post-paint probe AND forget-on-unmount.
    useEffect(() => {
      const tail = paintTailSignature(deferred[deferred.length - 1]?.msg);
      return scheduleAfterPaint(() => monitor.recordPaint(PANE, tail));
    }, [deferred]);
    useEffect(() => () => monitor.forget(PANE), []);
    return createElement("output", null, String(deferred.length));
  }

  const host = document.createElement("div");
  document.body.appendChild(host);

  // Mount (Chat view) and settle the initial snapshot.
  const root1 = createRoot(host);
  await act(async () => { root1.render(createElement(Harness)); });
  await flush();
  assert(monitor.snapshot(PANE)!.snapshotTail === "m0#2", "first snapshot recovers the seed transcript tail");

  // Stream a stable-id assistant turn; the backend transcript grows in lockstep.
  const streamCount = 24;
  await act(async () => {
    for (let i = 0; i < streamCount; i += 1) {
      const grown = msg("m1", "x".repeat(i + 1));
      backend[1] = grown;
      handler?.({
        payload: {
          pane_id: PANE, messages: [grown],
          source_session_id: "session-own", session_binding_nonce: "nonce-a",
        },
      });
    }
  });
  await flush();
  assert(monitor.snapshot(PANE)!.committedTail === `m1#${streamCount}`, "streaming committed the grown tail before teardown");

  // Terminal switch: unmount. forget() archives the prior committed tail.
  await act(async () => root1.unmount());
  await flush();
  assert(monitor.snapshot(PANE) === null, "unmount drops the live record");
  assert(monitor.priorActivation(PANE)!.committedTail === `m1#${streamCount}`, "prior committed tail survives real unmount teardown");

  // Back to Chat: remount re-runs chat_start, which replays the full transcript.
  cursor = "cur-1";
  const root2 = createRoot(host);
  await act(async () => { root2.render(createElement(Harness)); });
  await flush();
  assert(monitor.snapshot(PANE)!.snapshotTail === `m1#${streamCount}`, "reactivated snapshot recovered the full committed tail");
  assert(monitor.snapshot(PANE)!.snapshotCursor === "cur-1", "reactivated snapshot recorded the fresh backend cursor");
  assert(monitor.activationRecovery(PANE) === "recovered", "away→back reconcile is a clean signature-gated recovery end to end");

  await act(async () => root2.unmount());
  await flush();
  (bridge as never as { __setInvokeForTest: (fn: unknown) => void }).__setInvokeForTest(null as never);
  (bridge as never as { __setListenForTest: (fn: unknown) => void }).__setListenForTest(null as never);
}

async function main(): Promise<void> {
  testStageDistinction();
  testAuthorityStall();
  testMonotonicAndBounded();
  testForgetLifecycle();
  testScheduleAfterPaint();
  testFrozenTailStableIdStreaming();
  testSnapshotSignatureRecorded();
  testActivationReconcileOracle();
  testTestOnlyExposure();
  await testFrozenTailSignatureIntegration();
  await testActivationRemountIntegration();
  console.log("PASS chat projection diagnostics: stage distinction, bounded monotonic counters, lifecycle cleanup, post-paint probe, stable-id frozen-tail read seam, snapshot recovery coords, away→back reconcile oracle, test-only exposure");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
