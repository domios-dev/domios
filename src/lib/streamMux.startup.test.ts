import { afterEach, expect, it } from "vitest";
import { muxMockFetch } from "./muxMockFetch.testdata";
import { waitForDesktopReady } from "./serverReadiness";
import { __resetStreamMuxesForTest, withStreamMux } from "./streamMux";

afterEach(() => __resetStreamMuxesForTest());

// A cold start: the daemon refuses the mux session with a startup 503 until it is ready, as a launched server does.
it("a page loaded during daemon startup reaches desktop readiness without the carrier", async () => {
  let ready = false;
  const streams = new Set<ReadableStreamDefaultController<Uint8Array>>();
  const encode = (state: string) => new TextEncoder().encode(`data: ${JSON.stringify({ event_type: "startup-state", data: { state, subsystems: {} } })}\n\n`);
  const startupEvents = () => new Response(new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(encode(ready ? "ready" : "starting"));
      if (ready) controller.close(); else streams.add(controller);
    },
  }), { headers: { "Content-Type": "text/event-stream" } });
  const daemon = muxMockFetch((input: string) => new URL(input).pathname === "/api/startup/events" ? startupEvents() : Response.json({}));
  let muxCalls = 0;
  const fetch: typeof globalThis.fetch = async (input, init) => {
    const url = new URL(String(input));
    if (url.pathname.startsWith("/api/stream-mux")) muxCalls++;
    if (url.pathname === "/api/startup/events") return startupEvents();
    if (!ready && url.pathname.startsWith("/api/stream-mux")) {
      return Response.json({ error: { code: "starting", subsystem: "api", request_executed: false } }, { status: 503 });
    }
    return daemon(input, init);
  };
  const client = withStreamMux({ origin: "http://daemon.fixture", token: "fixture-token", fetch });
  const controller = new AbortController();
  const desktop = waitForDesktopReady(client, controller.signal);
  await new Promise(resolve => setTimeout(resolve, 20));
  ready = true;
  for (const stream of streams) { stream.enqueue(encode("ready")); stream.close(); }
  const settled = await Promise.race([desktop.then(() => "ready"), new Promise(resolve => setTimeout(() => resolve("stalled"), 2000))]);
  controller.abort();
  expect(settled).toBe("ready");
  expect(muxCalls).toBe(0);
});
