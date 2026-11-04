import { afterEach, describe, expect, it, vi } from "vitest";

const ORIGIN = window.location.origin;
const PEERS = 4;
const TABS_PER_PEER = 3;
const peerId = (index: number) => `928b06dd-1234-4567-890a-12345678900${index}`;
const remoteTab = (peer: number, tab: number) => `mesh:${peerId(peer)}/tab-${tab}`;

interface Channel { channel: string; path: string }

function fakeDaemon() {
  const longLived: Array<{ path: string; signal: AbortSignal | null | undefined }> = [];
  const opened: Channel[] = [];
  const released: string[] = [];
  const rpcs: Array<{ channel: string; method: string; path: string }> = [];
  let wire!: (frame: unknown) => void;
  const heads = new Set<string>();
  const push = (value: unknown) => {
    const frame = value as { channel: string; open?: boolean; content_type?: string; event?: string; data?: unknown; end?: unknown };
    const head = () => {
      if (heads.has(frame.channel)) return;
      heads.add(frame.channel);
      wire({ channel: frame.channel, open: true, status: 200, headers: [["content-type", frame.content_type ?? "text/event-stream"]] });
    };
    if (frame.open) { head(); return; }
    if ("data" in frame) {
      head();
      const event = frame.event ? "event: " + frame.event + "\n" : "";
      wire({ channel: frame.channel, chunk: event + "data: " + JSON.stringify(frame.data) + "\n\n", enc: "utf8" });
    } else wire(frame);
  };
  let drop!: () => void;
  const fetch = vi.fn(async (input: RequestInfo | URL, init: RequestInit = {}) => {
    const url = new URL(String(input));
    const method = init.method ?? "GET";
    if (url.pathname === "/api/stream-mux") {
      longLived.push({ path: url.pathname + url.search, signal: init.signal });
      if (url.pathname !== "/api/stream-mux") return new Response("unexpected stream", { status: 500 });
      const body = new ReadableStream<Uint8Array>({
        start(controller) {
          drop = () => controller.error(new Error("fixture mux loss"));
          wire = (frame) => controller.enqueue(new TextEncoder().encode(`data: ${JSON.stringify(frame)}\n\n`));
        },
      });
      return new Response(body, { headers: { "Content-Type": "text/event-stream" } });
    }
    if (method === "POST" && url.pathname.endsWith("/channels")) {
      opened.push(JSON.parse(String(init.body)) as Channel);
      return Response.json({ accepted: true }, { status: 202 });
    }
    if (method === "DELETE") {
      released.push(url.pathname.split("/").pop()!);
      return Response.json({ ok: true, closed: true });
    }
    const parts = url.pathname.split("/");
    if (method === "POST" && parts[2] === "stream-mux" && parts[4] === "requests") {
      const channel = parts[5];
      const headers = new Headers(init.headers);
      rpcs.push({ channel, method: headers.get("X-Domios-Mux-Method")!, path: headers.get("X-Domios-Mux-Path")! });
      // Early frames precede the ack, as on the real daemon.
      wire({ channel, open: true, status: 200, headers: [["content-type", "application/json"]] });
      const path = headers.get("X-Domios-Mux-Path")!;
      const answer = path === "/api/mesh/relay-to" ? { ok: true, status: 200, data: { data: channel } } : { data: channel };
      wire({ channel, chunk: JSON.stringify(answer), enc: "utf8" });
      wire({ channel, end: { reason: "eof" } });
      return Response.json({ accepted: true }, { status: 202 });
    }
    return new Response("UI request bypassed the mux", { status: 500 });
  });
  return { fetch, longLived, opened, released, rpcs, drop: () => drop(), push };
}

async function boot() {
  vi.resetModules();
  localStorage.clear();
  sessionStorage.clear();
  sessionStorage.setItem("codeterm-web-output-boot", "boot");
  const daemon = fakeDaemon();
  vi.stubGlobal("fetch", daemon.fetch);
  const bootstrap = await import("./daemonBootstrap");
  const remoteEvents = await import("./remoteTabEvents");
  const output = await import("./outputStream");
  const base = bootstrap.configureDaemonTransport({ origin: ORIGIN, token: "fixture" });
  remoteEvents.bindRemoteTabEventsTransport(base);
  output.bindOutputStreamTransport(base);
  return { daemon, remoteEvents, output };
}

const last = <T,>(items: T[]): T => items[items.length - 1];
const channelFor = (opened: Channel[], match: (path: URL) => boolean) =>
  last(opened.filter(({ path }) => match(new URL(path, ORIGIN)))).channel;

afterEach(async () => { (await import("./streamMux")).__resetStreamMuxesForTest(); vi.unstubAllGlobals(); });

describe("one multiplexed stream per origin", () => {
  it("events, agent-run and surface reconnect through one mux with exact event and byte cursors", async () => {
    const { daemon } = await boot();
    const webEvents = await import("./webEvents");
    const { baseDaemonTransport } = await import("./daemonBootstrap");
    const { subscribeAgentRunOutput } = await import("../components/AgentRun/agentRunOutput");
    const { createHttpSseSurfaceTransport } = await import("../components/Surface/httpSseSurfaceTransport");
    const eventPayloads: unknown[] = [];
    const agentOutput: string[] = [];
    const states: unknown[] = [];
    const resyncs: string[] = [];
    const { onResync } = await import("./resync");
    const stopResync = onResync(reason => resyncs.push(reason));
    const stopEvents = webEvents.webListen("fixture", ({ payload }) => eventPayloads.push(payload));
    const abort = new AbortController();
    const stopAgent = subscribeAgentRunOutput(baseDaemonTransport(), "agent", abort.signal, {
      output: text => agentOutput.push(text), connected() {}, ended() {},
    });
    const stopSurface = createHttpSseSurfaceTransport({ baseUrl: ORIGIN, token: "fixture", tabId: "surface", retrySignal: () => () => {} })
      .connect({ w: 640, h: 480, dpr: 2 }, { open() {}, state: value => states.push(value), frame() {}, notice() {} }, "viewer", "target");
    try {
      await vi.waitFor(() => expect(daemon.opened).toHaveLength(3));
      expect(daemon.longLived).toHaveLength(1);
      const eventChannel = channelFor(daemon.opened, url => url.pathname === "/api/events/stream");
      const agentChannel = channelFor(daemon.opened, url => url.pathname === "/api/tabs/agent/output/stream");
      const surfaceChannel = channelFor(daemon.opened, url => url.pathname === "/api/surfaces/surface/stream");
      for (const channel of [eventChannel, agentChannel, surfaceChannel]) daemon.push({ channel, open: true, content_type: "text/event-stream" });
      daemon.push({ channel: eventChannel, data: { boot: "boot", seq: 1, prev_seq: null, event_type: "fixture", data: "first" } });
      daemon.push({ channel: eventChannel, data: { boot: "boot", seq: 3, prev_seq: 1, event_type: "fixture", data: "after-private" } });
      daemon.push({ channel: agentChannel, data: { output: "雪", offset: 3 } });
      daemon.push({ channel: surfaceChannel, event: "state", data: { owner: "fixture" } });
      await vi.waitFor(() => expect(eventPayloads).toEqual(["first", "after-private"]));
      await vi.waitFor(() => expect(agentOutput).toEqual(["雪"]));
      await vi.waitFor(() => expect(states).toEqual([{ owner: "fixture" }]));
      expect(webEvents.webEventSeqGaps()).toBe(0);
      // The receive loop announces its reconnect wait before it accepts a wake.
      const { TERMINAL_VIEWER_DISCONNECTED_EVENT } = await import("./terminalViewerEvents");
      const disconnected = new Promise<void>(resolve => window.addEventListener(TERMINAL_VIEWER_DISCONNECTED_EVENT, () => resolve(), { once: true }));
      daemon.drop();
      await disconnected;
      webEvents.wakeWebEvents();
      await vi.waitFor(() => expect(daemon.opened).toHaveLength(6));
      expect(daemon.longLived).toHaveLength(2);
      expect(daemon.longLived.every(({ path }) => new URL(path, ORIGIN).pathname === "/api/stream-mux")).toBe(true);
      const resumedEvents = last(daemon.opened.filter(({ path }) => new URL(path, ORIGIN).pathname === "/api/events/stream"));
      const resumedAgent = last(daemon.opened.filter(({ path }) => new URL(path, ORIGIN).pathname === "/api/tabs/agent/output/stream"));
      const resumedSurface = last(daemon.opened.filter(({ path }) => new URL(path, ORIGIN).pathname === "/api/surfaces/surface/stream"));
      expect(new URL(resumedEvents.path, ORIGIN).searchParams.get("since")).toBe("boot:3");
      expect(new URL(resumedAgent.path, ORIGIN).searchParams.get("from")).toBe("3");
      expect(resumedSurface.path).toBe(daemon.opened.find(({ channel }) => channel === surfaceChannel)!.path);
      daemon.push({ channel: resumedEvents.channel, data: { boot: "boot", seq: 3, prev_seq: 1, event_type: "fixture", data: "duplicate" } });
      daemon.push({ channel: resumedEvents.channel, data: { boot: "boot", seq: 5, prev_seq: 3, event_type: "fixture", data: "resumed" } });
      daemon.push({ channel: resumedAgent.channel, data: { output: "!", offset: 4 } });
      await vi.waitFor(() => expect(eventPayloads).toEqual(["first", "after-private", "resumed"]));
      await vi.waitFor(() => expect(agentOutput).toEqual(["雪", "!"]));
      expect(webEvents.webEventSeqGaps()).toBe(0);
      expect(resyncs).toEqual([]);
      daemon.push({ channel: resumedEvents.channel, data: { boot: "boot", seq: 9, prev_seq: 8, event_type: "fixture", data: "real-loss" } });
      await vi.waitFor(() => expect(webEvents.webEventSeqGaps()).toBe(1));
      expect(resyncs).toEqual(["overflow"]);
    } finally {
      stopResync();
      stopEvents();
      webEvents.__resetWebEventsForTest();
      stopAgent();
      abort.abort();
      stopSurface();
    }
  });

  it("N peers x M remote tabs plus a local terminal hold exactly one long-lived connection", async () => {
    const { daemon, remoteEvents, output } = await boot();
    const stops: Array<() => void> = [];
    const chat: unknown[] = [];
    for (let peer = 0; peer < PEERS; peer++) {
      for (let tab = 0; tab < TABS_PER_PEER; tab++) {
        const id = remoteTab(peer, tab);
        stops.push(remoteEvents.subscribeRemoteTabEvent(`tab-chat-msg-${id}`, ({ payload }) => { chat.push(payload); })!);
        stops.push(remoteEvents.subscribeRemoteTabEvent(`agent-runtime-state:${id}`, () => {})!);
        stops.push(remoteEvents.subscribeRemoteTabEvent(`pty-resize-owner-changed:${id}`, () => {})!);
        stops.push(remoteEvents.subscribeRemoteTabEvent(`peer-state-changed:${id}`, () => {})!);
        stops.push(output.subscribeTabOutput(id, { onOutput() {}, onTruncated() {}, onEnded() {} }));
      }
    }
    const localOutput = vi.fn();
    stops.push(output.subscribeTabOutput("local-tab", { onOutput: localOutput, onTruncated() {}, onEnded() {} }, {}, "local-tab"));

    const remoteChannels = PEERS * TABS_PER_PEER * 2;
    await vi.waitFor(() => expect(daemon.opened.length).toBe(remoteChannels + 1));
    expect(daemon.longLived.map(({ path }) => new URL(path, ORIGIN).pathname)).toEqual(["/api/stream-mux"]);

    const target = remoteTab(1, 2);
    const eventsChannel = channelFor(daemon.opened, (url) => url.searchParams.get("tab") === target
      && url.searchParams.get("path") === "/api/events/stream");
    daemon.push({ channel: eventsChannel, open: true, content_type: "text/event-stream" });
    daemon.push({ channel: eventsChannel, data: { boot: "peer-boot", seq: 7, event_type: "tab-chat-msg-tab-2", data: { delta: "hi" } } });
    const localChannel = channelFor(daemon.opened, (url) => url.pathname === "/api/output/stream");
    daemon.push({ channel: localChannel, open: true, content_type: "text/event-stream" });
    daemon.push({ channel: localChannel, data: { tab_id: "local-tab", pty_id: "local-tab", boot: "boot", offset: 1, b64: "QQ" } });
    await vi.waitFor(() => expect(chat).toEqual([{ delta: "hi", tab_id: target }]));
    await vi.waitFor(() => expect(localOutput).toHaveBeenCalledWith("A"));
    expect(remoteEvents.remoteTabEventSubscriptionSnapshot(`tab-chat-msg-${target}`)?.cursor).toBe("peer-boot:7");

    const before = daemon.opened.length;
    daemon.push({ channel: eventsChannel, end: { reason: "eof" } });
    await vi.waitFor(() => expect(daemon.opened.length).toBe(before + 1));
    const resumed = new URL(last(daemon.opened).path, ORIGIN);
    expect(resumed.searchParams.get("tab")).toBe(target);
    expect(resumed.searchParams.get("since")).toBe("peer-boot:7");
    expect(daemon.longLived).toHaveLength(1);

    for (const stop of stops) stop();
    output.resetOutputStream();
    expect(daemon.longLived[0].signal?.aborted).toBe(false);
    expect(daemon.longLived).toHaveLength(1);
  });

  it("N peers x M remote tabs x K previews plus events answer over one long-lived connection", async () => {
    const { daemon, remoteEvents } = await boot();
    const { daemonTransportForPeer, baseDaemonTransport } = await import("./daemonBootstrap");
    const webEvents = await import("./webEvents");
    const stops = [webEvents.webListen("fixture", () => {})];
    for (let peer = 0; peer < PEERS; peer++) {
      for (let tab = 0; tab < TABS_PER_PEER; tab++) {
        stops.push(remoteEvents.subscribeRemoteTabEvent(`tab-chat-msg-${remoteTab(peer, tab)}`, () => {})!);
      }
    }
    const previews = 24;
    const reads = Array.from({ length: previews }, (_, index) => baseDaemonTransport()
      .json<{ data: string }>("POST", `/api/tabs/local/preview/read`, { path: `card-${index}.png`, kind: "image" }));
    const peerReads = Array.from({ length: PEERS }, (_, peer) => daemonTransportForPeer(peerId(peer))
      .json<{ data: string }>("GET", "/api/tabs"));
    const answers = await Promise.all([...reads, ...peerReads]);
    expect(answers.every(answer => typeof answer.data === "string")).toBe(true);
    expect(daemon.rpcs.filter(({ path }) => path === "/api/mesh/relay-to")).toHaveLength(PEERS);
    expect(daemon.rpcs.filter(({ path }) => path.endsWith("/preview/read"))).toHaveLength(previews);
    expect(daemon.longLived.map(({ path }) => new URL(path, ORIGIN).pathname)).toEqual(["/api/stream-mux"]);
    for (const stop of stops) stop();
    webEvents.__resetWebEventsForTest();
  });

  it("closing one remote tab releases only its channels and keeps the shared stream", async () => {
    const { daemon, remoteEvents, output } = await boot();
    const keep = remoteTab(0, 0);
    const close = remoteTab(1, 0);
    const keepStops = [
      remoteEvents.subscribeRemoteTabEvent(`tab-chat-msg-${keep}`, () => {})!,
      output.subscribeTabOutput(keep, { onOutput() {}, onTruncated() {}, onEnded() {} }),
    ];
    const closeStops = [
      remoteEvents.subscribeRemoteTabEvent(`tab-chat-msg-${close}`, () => {})!,
      output.subscribeTabOutput(close, { onOutput() {}, onTruncated() {}, onEnded() {} }),
    ];
    await vi.waitFor(() => expect(daemon.opened).toHaveLength(4));
    const closing = daemon.opened.filter(({ path }) => new URL(path, ORIGIN).searchParams.get("tab") === close).map(({ channel }) => channel);
    for (const stop of closeStops) stop();
    await vi.waitFor(() => expect(daemon.released.sort()).toEqual([...closing].sort()));
    expect(daemon.longLived).toHaveLength(1);
    expect(daemon.longLived[0].signal?.aborted).toBe(false);
    for (const stop of keepStops) stop();
    expect(daemon.longLived[0].signal?.aborted).toBe(false);
    output.resetOutputStream();
  });

  it("opening a local terminal tab rides the shared stream instead of a new connection", async () => {
    const { daemon, output } = await boot();
    const written = vi.fn();
    const stop = output.subscribeTabOutput("terminal-tab", { onOutput: written, onTruncated() {}, onEnded() {} }, {}, "terminal-tab");
    await vi.waitFor(() => expect(daemon.opened).toHaveLength(1));
    expect(new URL(daemon.opened[0].path, ORIGIN).pathname).toBe("/api/output/stream");
    expect(daemon.longLived.map(({ path }) => new URL(path, ORIGIN).pathname)).toEqual(["/api/stream-mux"]);
    daemon.push({ channel: daemon.opened[0].channel, data: { tab_id: "terminal-tab", pty_id: "terminal-tab", boot: "boot", offset: 2, b64: "T0s" } });
    await vi.waitFor(() => expect(written).toHaveBeenCalledWith("OK"));
    stop();
    output.resetOutputStream();
  });
});

