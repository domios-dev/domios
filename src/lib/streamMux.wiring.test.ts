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
  let push!: (frame: unknown) => void;
  const fetch = vi.fn(async (input: RequestInfo | URL, init: RequestInit = {}) => {
    const url = new URL(String(input));
    const method = init.method ?? "GET";
    if (new Headers(init.headers).get("Accept") === "text/event-stream") {
      longLived.push({ path: url.pathname + url.search, signal: init.signal });
      if (url.pathname !== "/api/stream-mux") return new Response("unexpected stream", { status: 500 });
      const body = new ReadableStream<Uint8Array>({
        start(controller) {
          push = (frame) => controller.enqueue(new TextEncoder().encode(`data: ${JSON.stringify(frame)}\n\n`));
        },
      });
      return new Response(body, { headers: { "Content-Type": "text/event-stream" } });
    }
    if (method === "POST" && url.pathname.endsWith("/channels")) {
      opened.push(JSON.parse(String(init.body)) as Channel);
      return Response.json({ ok: true });
    }
    if (method === "DELETE") {
      released.push(url.pathname.split("/").pop()!);
      return Response.json({ ok: true, closed: true });
    }
    return Response.json({ ok: true });
  });
  return { fetch, longLived, opened, released, push: (frame: unknown) => push(frame) };
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

afterEach(() => { vi.unstubAllGlobals(); });

describe("one multiplexed stream per origin", () => {
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
    expect(daemon.longLived[0].signal?.aborted).toBe(true);
    expect(daemon.longLived).toHaveLength(1);
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
    expect(daemon.longLived[0].signal?.aborted).toBe(true);
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
