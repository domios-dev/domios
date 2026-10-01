import assert from "node:assert/strict";
import { Window } from "happy-dom";
import { withFailureDeadline } from "../test/withFailureDeadline";

async function main() {
  const win = new Window({ url: "http://gateway:7685" });
  Object.assign(globalThis, {
    window: win, localStorage: win.localStorage, sessionStorage: win.sessionStorage,
    Event: win.Event, CustomEvent: win.CustomEvent,
  });
  const { setRelayTarget, clearRelayTarget } = await import("./webApi");
  type Connection = { url: URL; controller: ReadableStreamDefaultController<Uint8Array> };
  const connections: Connection[] = [];
  const allConnections: Connection[] = [];
  let awaitConnection: ((connection: Connection) => void) | null = null;
  globalThis.fetch = async (input) => {
    let controller!: ReadableStreamDefaultController<Uint8Array>;
    const body = new ReadableStream<Uint8Array>({ start(value) { controller = value; } });
    const connection = { url: new URL(String(input)), controller };
    allConnections.push(connection);
    if (awaitConnection) {
      const resolve = awaitConnection;
      awaitConnection = null;
      resolve(connection);
    } else connections.push(connection);
    return new Response(body);
  };
  const nextConnection = () => withFailureDeadline(connections.length
    ? Promise.resolve(connections.shift()!)
    : new Promise<Connection>((resolve) => { awaitConnection = resolve; }), "SSE connection");

  const { webListen, resetWebEvents, __resetWebEventsForTest } = await import("./webEvents");
  const { TERMINAL_VIEWER_RECONNECTED_EVENT } = await import("./terminalViewerEvents");
  let reconnects = 0;
  const reconnected = () => { reconnects += 1; };
  win.addEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, reconnected);
  const received: unknown[] = [];
  let awaitFrame: (() => void) | null = null;
  const unlisten = webListen("plan-changed", ({ payload }) => {
    received.push(payload);
    awaitFrame?.();
    awaitFrame = null;
  });
  const encoder = new TextEncoder();
  function push(connection: Connection, seq: number, planId: string) {
    connection.controller.enqueue(encoder.encode(`data: ${JSON.stringify({ boot: "boot", seq, event_type: "plan-changed", data: { plan_id: planId } })}\n\n`));
  }
  async function deliver(connection: Connection, seq: number, planId: string) {
    const delivered = new Promise<void>((resolve) => { awaitFrame = resolve; });
    push(connection, seq, planId);
    await withFailureDeadline(delivered, `plan-changed frame ${planId}`);
  }

  try {
    const local = await nextConnection();
    assert.equal(local.url.pathname, "/api/events/stream");
    await deliver(local, 10, "pl_local");

    setRelayTarget("ubunta", "ubunta");
    resetWebEvents();
    const peer = await nextConnection();
    assert.equal(peer.url.pathname, "/api/mesh/relay-stream");
    assert.equal(peer.url.searchParams.get("peer"), "ubunta");
    assert.equal(peer.url.searchParams.get("path"), "/api/events/stream");
    assert.equal(peer.url.searchParams.get("since"), null, "the local cursor must never follow the browser onto a peer");
    push(local, 999, "pl_late_local");
    await deliver(peer, 20, "pl_peer");

    clearRelayTarget();
    resetWebEvents();
    const returned = await nextConnection();
    assert.equal(returned.url.pathname, "/api/events/stream");
    assert.equal(returned.url.searchParams.get("since"), "boot:10", "switching back resumes only the local cursor");
    push(peer, 999, "pl_late_peer");
    await deliver(returned, 11, "pl_local_updated");
    assert.equal(reconnects, 2, "both peer switches dispatch a DOM reconnect event");
    assert.deepEqual(received, [{ plan_id: "pl_local" }, { plan_id: "pl_peer" }, { plan_id: "pl_local_updated" }]);
  } finally {
    unlisten();
    __resetWebEventsForTest();
    win.removeEventListener(TERMINAL_VIEWER_RECONNECTED_EVENT, reconnected);
    awaitConnection = null;
    awaitFrame = null;
    for (const connection of allConnections) connection.controller.close();
  }
  console.log("webEvents peer isolation passed");
}

void main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
