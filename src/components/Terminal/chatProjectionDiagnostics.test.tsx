// Run: node node_modules/tsx/dist/cli.cjs src/components/Terminal/chatProjectionDiagnostics.test.tsx
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import {
  act,
  createElement,
  useDeferredValue,
  useLayoutEffect,
  useState,
} from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { ChatProjectionMonitor } from "./chatProjectionDiagnostics";
import type { ChatSessionDiagnosticEvent } from "./useChatSession";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

const PANE = "pane";
// Default run documents the CURRENT (starved) contract and is green. Setting
// CHAT_PROJECTION_ASSERT_FIXED=1 flips the streaming assertions to the
// post-correction contract, which fails at the current head — the reproducible
// fails-before baseline a later rendering fix is expected to turn green.
const ASSERT_FIXED = process.env.CHAT_PROJECTION_ASSERT_FIXED === "1";

function arrival(paneId: string, messageIds: string[]): ChatSessionDiagnosticEvent {
  return { edge: "event_arrival", paneId, sourceSessionId: null, sessionBindingNonce: null, messageIds };
}
function adopt(paneId: string, messageIds: string[]): ChatSessionDiagnosticEvent {
  return { edge: "authority_adopt", paneId, sourceSessionId: null, sessionBindingNonce: null, messageIds };
}
function drop(paneId: string, messageIds: string[]): ChatSessionDiagnosticEvent {
  return { edge: "authority_drop", paneId, sourceSessionId: null, sessionBindingNonce: null, messageIds };
}
function commit(paneId: string, messageIds: string[]): ChatSessionDiagnosticEvent {
  return { edge: "state_commit", paneId, sourceSessionId: null, sessionBindingNonce: null, messageIds };
}

// ── Unit: distinct per-stage counters ──────────────────────────────────────
function testStageDistinction(): void {
  const monitor = new ChatProjectionMonitor();
  monitor.ingest(arrival(PANE, ["m0", "m1"]));
  monitor.ingest(adopt(PANE, ["m0", "m1"]));
  monitor.ingest(commit(PANE, ["m0", "m1"]));
  monitor.recordPaint(PANE, "m1");

  const snap = monitor.snapshot(PANE)!;
  assert(snap.producedMessages === 2, "producer counter sums delivered messages");
  assert(snap.arrived === 1, "backend-emit counter counts delivered delta events");
  assert(snap.adopted === 1 && snap.dropped === 0, "authority distinguishes adopt from drop");
  assert(snap.committed === 1, "committed-state counter distinct from arrival");
  assert(snap.painted === 1, "painted-tail counter distinct from committed state");
  assert(snap.arrivalTail === "m1" && snap.committedTail === "m1" && snap.paintedTail === "m1", "tails track newest id");
  assert(monitor.firstStalledEdge(PANE) === null, "fully caught-up chain reports no stall");
}

// ── Unit: authority drop surfaces as an arrival→authority gap ───────────────
function testAuthorityStall(): void {
  const monitor = new ChatProjectionMonitor();
  monitor.ingest(arrival(PANE, ["own"]));
  monitor.ingest(adopt(PANE, ["own"]));
  monitor.ingest(commit(PANE, ["own"]));
  monitor.recordPaint(PANE, "own");
  // A foreign-session delta arrives and is rejected: arrival tail advances, authority does not.
  monitor.ingest(arrival(PANE, ["foreign"]));
  monitor.ingest(drop(PANE, ["foreign"]));

  const snap = monitor.snapshot(PANE)!;
  assert(snap.dropped === 1 && snap.arrivalTail === "foreign", "rejected delta advances arrival, not authority");
  const edge = monitor.firstStalledEdge(PANE);
  assert(edge?.from === "backend_emit" && edge?.to === "frontend_authority", "authority rejection is the first stalled edge");
}

// ── Unit: monotonic + bounded ───────────────────────────────────────────────
function testMonotonicAndBounded(): void {
  const monitor = new ChatProjectionMonitor(2);
  for (let i = 0; i < 5; i += 1) monitor.ingest(arrival(PANE, [`m${i}`]));
  assert(monitor.snapshot(PANE)!.arrived === 5, "arrival counter only increases");

  monitor.ingest(arrival("a", ["x"]));
  monitor.ingest(arrival("b", ["y"]));
  assert(monitor.trackedPaneCount() === 2, "tracked panes capped at bound");
  monitor.ingest(arrival(PANE, ["z"])); // touch pane -> evicts the LRU pane ("a")
  assert(monitor.trackedPaneCount() === 2, "adding beyond the cap evicts, stays bounded");
  assert(monitor.snapshot("a") === null, "least-recently-used pane evicted");
  assert(monitor.snapshot(PANE) !== null && monitor.snapshot("b") !== null, "recently-used panes retained");
}

// ── Unit: the frozen-tail signature via synthetic streaming ─────────────────
function testFrozenTailSignatureSynthetic(): void {
  const monitor = new ChatProjectionMonitor();
  // Pre-load steady state: everything caught up at "m2".
  monitor.ingest(arrival(PANE, ["m0", "m1", "m2"]));
  monitor.ingest(adopt(PANE, ["m0", "m1", "m2"]));
  monitor.ingest(commit(PANE, ["m0", "m1", "m2"]));
  monitor.recordPaint(PANE, "m2");
  assert(monitor.firstStalledEdge(PANE) === null, "baseline steady state has no stalled edge");

  // Urgent streaming load: arrival→adopt→commit each advance, paint is starved.
  const streamCount = 48;
  for (let i = 0; i < streamCount; i += 1) {
    const id = `m${i + 3}`;
    monitor.ingest(arrival(PANE, [id]));
    monitor.ingest(adopt(PANE, [id]));
    monitor.ingest(commit(PANE, [id]));
    // No recordPaint — the downstream projection never runs during urgent load.
    const edge = monitor.firstStalledEdge(PANE);
    assert(
      edge?.from === "committed_state" && edge?.to === "painted_tail",
      `frozen tail: state committed to ${id} but paint stuck at ${edge?.observedTail}`,
    );
  }

  const loaded = monitor.snapshot(PANE)!;
  assert(loaded.committed === streamCount + 1, "every streamed delta committed state");
  assert(loaded.painted === 1, "paint stayed frozen through the whole urgent load");
  assert(loaded.committedTail === "m50" && loaded.paintedTail === "m2", "committed tail advanced while painted tail froze");

  // Release: one collapsed paint catches the visible tail up to the committed tail.
  monitor.recordPaint(PANE, "m50");
  assert(monitor.firstStalledEdge(PANE) === null, "a single collapsed paint clears the stall (not mistaken for lag)");
  assert(monitor.snapshot(PANE)!.painted === 2, "collapsed catch-up is one paint, not per-delta");
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

function msg(id: string): NormalizedChatMessage {
  return { id, type: "assistant", content: id, timestamp: id.slice(1) || "0" };
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
    useLayoutEffect(() => {
      monitor.recordPaint(PANE, deferred[deferred.length - 1]?.msg.id ?? null);
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

  const streamCount = 48;
  let committedDuringLoad = -1;
  let committedTailDuringLoad: string | null = null;
  let paintedTailDuringLoad: string | null = null;
  await act(async () => {
    for (let i = 0; i < streamCount; i += 1) {
      handler?.({
        payload: {
          pane_id: PANE, messages: [msg(`m${i + 1}`)],
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
  assert(committedTailDuringLoad === "m48", "integration: committed tail reached the newest delta");
  if (ASSERT_FIXED) {
    assert(paintedTailDuringLoad === committedTailDuringLoad, "corrected contract: painted tail stayed level with committed tail during load");
  } else {
    // Frozen-tail signature: the visible projection never caught the committed tail
    // during urgent load (the per-delta edge checks proved the lag held throughout).
    assert(paintedTailDuringLoad !== committedTailDuringLoad, "integration: painted tail lagged the committed tail through urgent load");
  }

  // Release urgent load: the deferred projection catches up in one collapsed paint.
  await flush();
  assert(monitor.firstStalledEdge(PANE) === null, "integration: paint catches up and clears the stall after load");
  assert(monitor.snapshot(PANE)!.paintedTail === "m48", "integration: painted tail catches up to committed tail");

  await act(async () => root.unmount());
  await flush();
  (bridge as never as { __setInvokeForTest: (fn: unknown) => void }).__setInvokeForTest(null as never);
  (bridge as never as { __setListenForTest: (fn: unknown) => void }).__setListenForTest(null as never);
}

async function main(): Promise<void> {
  testStageDistinction();
  testAuthorityStall();
  testMonotonicAndBounded();
  testFrozenTailSignatureSynthetic();
  await testFrozenTailSignatureIntegration();
  console.log("PASS chat projection diagnostics: stage distinction, bounded monotonic counters, frozen-tail read seam");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
