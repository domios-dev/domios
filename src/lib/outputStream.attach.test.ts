import { afterEach, describe, expect, it, vi } from "vitest";
import { createTransportClient } from "./transportClient";
import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import capture from "./outputStream.capture.fixture.json";
import { writeTerminalOutput } from "../hooks/terminal/terminalOutputWrite";

function pending<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

afterEach(() => { vi.unstubAllGlobals(); });

describe("aggregate terminal attachment", () => {
  it.each(["tab", "mesh:928b06dd-1234-4567-890a-123456789abc/tab"])("reports an empty %s replay only for the bound PTY", async tabId => {
    vi.resetModules();
    localStorage.clear();
    sessionStorage.setItem("codeterm-web-output-boot", "boot");
    const output = await import("./outputStream");
    let receive!: (name: string, frame: unknown) => void;
    output.bindOutputStreamTransport({ origin: () => "http://daemon.test", subscribe: (_path, handler) => {
      receive = handler; return () => {};
    } } as import("./transportClient").TransportClient);
    const written = vi.fn(), ready = vi.fn();
    const stop = output.subscribeTabOutput(tabId, {
      onOutput: written, onReplayComplete: ready, onTruncated() {}, onEnded() {},
    }, {}, "own-pty");
    try {
      receive("message", { tab_id: "tab", pty_id: "foreign-pty", boot: "boot", replay_complete: true, offset: 0 });
      expect(ready).not.toHaveBeenCalled();
      receive("message", { tab_id: "tab", pty_id: "own-pty", boot: "boot", replay_complete: true, offset: 0 });
      expect(ready).toHaveBeenCalledOnce();
      const cachedReady = vi.fn();
      const detach = output.subscribeTabOutput(tabId, {
        onOutput: written, onReplayComplete: cachedReady, onTruncated() {}, onEnded() {},
      }, {}, "own-pty");
      expect(cachedReady).toHaveBeenCalledOnce();
      detach();
      receive("message", { tab_id: "tab", pty_id: "foreign-pty", boot: "boot", offset: 1, b64: "WA" });
      expect(written).not.toHaveBeenCalled();
    } finally { stop(); output.resetOutputStream(); }
  });
  it.each(["tab", "mesh:928b06dd-1234-4567-890a-123456789abc/tab"])("rejects foreign PTY bytes before advancing %s replay", async tabId => {
    vi.resetModules();
    localStorage.clear();
    sessionStorage.setItem("codeterm-web-output-boot", "boot");
    const output = await import("./outputStream");
    let receive!: (name: string, frame: unknown) => void;
    output.bindOutputStreamTransport({ origin: () => "http://daemon.test", subscribe: (_path, handler) => {
      receive = handler; return () => {};
    } } as import("./transportClient").TransportClient);
    const written = vi.fn();
    const stop = output.subscribeTabOutput(tabId, { onOutput: written, onTruncated() {}, onEnded() {} }, {}, "own-pty");
    try {
      receive("message", { tab_id: "tab", pty_id: "foreign-pty", boot: "boot", offset: 99, b64: "WA" });
      receive("message", { tab_id: "foreign-tab", pty_id: "own-pty", boot: "boot", offset: 99, b64: "WA" });
      receive("message", { tab_id: "tab", pty_id: "own-pty", boot: "boot", offset: 1, b64: "QQ" });
      expect(written).toHaveBeenCalledExactlyOnceWith("A");
    } finally { stop(); output.resetOutputStream(); }
  });
  it("adopts truncation before any output arrives and reconnects twice without another marker", async () => {
    vi.resetModules();
    localStorage.clear();
    sessionStorage.setItem("codeterm-web-output-boot", "boot");
    const output = await import("./outputStream");
    let receive!: (name: string, frame: unknown) => void;
    let ends!: (end: import("./transportClient").StreamEnd) => void;
    const paths: string[] = [];
    output.bindOutputStreamTransport({ origin: () => "http://daemon.test", subscribe: (path, handler, options) => {
      paths.push(path); receive = handler; ends = options!.onEnd!; return () => {};
    } } as import("./transportClient").TransportClient);
    const gaps: number[] = [];
    const bytes: number[] = [];
    const terminal = {};
    const sink = () => ({ onOutput() {}, onBytes(chunk: Uint8Array) { bytes.push(...chunk); },
      onTruncated(from: number) { gaps.push(from); }, onEnded() {} });
    let detach = output.subscribeTabOutput("tab", sink(), terminal, "pty");
    try {
      receive("message", { tab_id: "tab", truncated: true, from: 42295516 });
      for (let index = 0; index < 2; index++) {
        ends({ reason: "eof" });
        output.pokeOutputStream();
        const since = new URL(paths[paths.length - 1], "http://daemon.test").searchParams.get("since")!;
        expect(JSON.parse(atob(since.replace(/-/g, "+").replace(/_/g, "/")))).toEqual({ boot: "boot", tabs: { tab: 42295516 } });
        receive("message", { tab_id: "tab", truncated: true, from: 42295516 });
      }
      expect(gaps).toEqual([42295516]);
      expect(paths).toHaveLength(3);
      detach();
      detach = output.subscribeTabOutput("tab", sink(), terminal, "pty");
      receive("message", { tab_id: "tab", truncated: true, from: 42295516 });
      receive("message", { tab_id: "tab", pty_id: "pty", boot: "boot", b64: "QQ", offset: 42295517 });
      expect(gaps).toEqual([42295516]);
      expect(bytes).toEqual([65]);
      receive("message", { tab_id: "tab", truncated: true, from: 42295600 });
      expect(gaps).toEqual([42295516, 42295600]);
    } finally { detach(); output.resetOutputStream(); }
  });

  it("adopts remote truncation before reporting it and suppresses repeat control frames on forced resync", async () => {
    vi.resetModules();
    localStorage.clear();
    const output = await import("./outputStream");
    let receive!: (name: string, frame: unknown) => void;
    const paths: string[] = [];
    output.bindOutputStreamTransport({ origin: () => "http://daemon.test", subscribe: (path, handler, options) => {
      paths.push(path); receive = handler;
      options!.signal!.addEventListener("abort", () => options!.onEnd!({ reason: "eof" }), { once: true });
      return () => {};
    } } as import("./transportClient").TransportClient);
    const gaps: number[] = [];
    const detach = output.subscribeTabOutput("mesh:928b06dd-1234-4567-890a-123456789abc/tab", {
      onOutput() {}, onEnded() {}, onTruncated(from) {
        gaps.push(from);
        output.resumeRemoteOutputStreams();
      },
    }, {}, "pty");
    try {
      receive("message", { boot: "boot" });
      receive("message", { tab_id: "foreign", truncated: true, from: 999 });
      receive("message", { tab_id: "tab", truncated: true, from: 100 });
      expect(paths).toHaveLength(2);
      const cursor = () => {
        const since = new URL(paths[paths.length - 1], "http://daemon.test").searchParams.get("since")!;
        return JSON.parse(atob(since.replace(/-/g, "+").replace(/_/g, "/")));
      };
      expect(cursor()).toEqual({ boot: "boot", tabs: { tab: 100 } });
      receive("message", { tab_id: "tab", truncated: true, from: 100 });
      output.resumeRemoteOutputStreams();
      expect(cursor()).toEqual({ boot: "boot", tabs: { tab: 100 } });
      receive("message", { tab_id: "tab", truncated: true, from: 100 });
      expect(gaps).toEqual([100]);
      expect(paths).toHaveLength(3);
    } finally { detach(); output.resetOutputStream(); }
  });

  it("waits after idle output EOF and resumes only after the event stream proves recovery", async () => {
    vi.resetModules();
    const output = await import("./outputStream");
    const { TERMINAL_VIEWER_RECONNECTED_EVENT } = await import("./terminalViewerEvents");
    let connected = pending<void>();
    let ends!: (end: import("./transportClient").StreamEnd) => void;
    let receive!: (name: string, data: unknown) => void;
    let subscriptions = 0;
    output.bindOutputStreamTransport({ origin: () => "http://daemon.test", subscribe: (_path, handler, options) => {
      receive = handler; ends = options!.onEnd!; subscriptions++; connected.resolve(); return () => {};
    } } as import("./transportClient").TransportClient);
    const bytes: number[] = [];
    const detach = output.subscribeTabOutput("idle", { onOutput() {}, onBytes(chunk) { bytes.push(...chunk); }, onTruncated() {}, onEnded() {} });
    try {
      await connected.promise;
      connected = pending<void>();
      ends({ reason: "eof" });
      await Promise.resolve();
      expect(subscriptions).toBe(1);
      window.dispatchEvent(new Event(TERMINAL_VIEWER_RECONNECTED_EVENT));
      await connected.promise;
      expect(subscriptions).toBe(2);
      ends({ reason: "http", status: 503 });
      await Promise.resolve();
      connected = pending<void>();
      window.dispatchEvent(new Event(TERMINAL_VIEWER_RECONNECTED_EVENT));
      await connected.promise;
      expect(subscriptions).toBe(3);
      receive("message", { tab_id: "idle", pty_id: "idle", boot: "next", offset: 1, b64: "QQ" });
      expect(bytes).toEqual([65]);
    } finally { detach(); output.resetOutputStream(); }
  });
  it("shares one live aggregate connection across staggered mounts, remounts and resync signals", async () => {
    vi.resetModules();
    localStorage.clear();
    sessionStorage.setItem("codeterm-web-output-boot", "boot");
    const output = await import("./outputStream");
    const { TERMINAL_VIEWER_RECONNECTED_EVENT } = await import("./terminalViewerEvents");
    let receive!: (name: string, frame: unknown) => void;
    const subscribe = vi.fn((_path, handler, _options) => { receive = handler; return () => {}; });
    output.bindOutputStreamTransport({ origin: () => "http://daemon.test", subscribe } as unknown as import("./transportClient").TransportClient);
    const written = Array.from({ length: 20 }, () => vi.fn());
    const stops: (() => void)[] = [];
    for (let index = 0; index < 20; index++) {
      stops.push(output.subscribeTabOutput(`tab-${index}`, {
        onOutput: written[index], onTruncated() {}, onEnded() {},
      }));
      await Promise.resolve();
      window.dispatchEvent(new Event(TERMINAL_VIEWER_RECONNECTED_EVENT));
    }
    expect(subscribe).toHaveBeenCalledTimes(1);
    receive("message", { tab_id: "tab-4", pty_id: "tab-4", boot: "boot", b64: "ZXhhY3Q", offset: 5 });
    expect(written[4]).toHaveBeenCalledExactlyOnceWith("exact");
    expect(written.filter(callback => callback.mock.calls.length)).toHaveLength(1);
    stops.forEach(stop => stop());
    const replayed = vi.fn();
    const stop = output.subscribeTabOutput("tab-4", { onOutput: replayed, onTruncated() {}, onEnded() {} });
    expect(subscribe).toHaveBeenCalledTimes(1);
    expect(replayed).toHaveBeenCalledExactlyOnceWith("exact");
    stop();
    output.resetOutputStream();
  });
  it("replays a new terminal and resumes a retained terminal without duplicate bytes or PTY mutations", async () => {
    vi.resetModules();
    sessionStorage.setItem("codeterm-web-output-boot", "boot");
    localStorage.clear();
    let token = "test-token";
    let nonce = "exact-nonce";
    const requests: Array<{ url: URL; signal: AbortSignal }> = [];
    let connection = pending<ReadableStreamDefaultController<Uint8Array>>();
    vi.stubGlobal("fetch", async (input: string, init: RequestInit) => {
      const signal = init.signal as AbortSignal;
      requests.push({ url: new URL(input), signal });
      expect(new Headers(init.headers).get("Authorization")).toBe(`Bearer ${token}`);
      expect(new Headers(init.headers).get("X-CodeTerm-Session-Binding-Nonce")).toBe(nonce);
      return new Response(new ReadableStream<Uint8Array>({
        start(controller) {
          signal.addEventListener("abort", () => controller.error(new DOMException("Detached", "AbortError")), { once: true });
          connection.resolve(controller);
        },
      }));
    });
    const output = await import("./outputStream");
    output.bindOutputStreamTransport(createTransportClient({ origin: "http://daemon.test", token: "test-token", headers: { "X-CodeTerm-Session-Binding-Nonce": "exact-nonce" } }));
    const desktop = {}, browser = {};
    const bytes: number[][] = [[], []];
    let written = pending<void>();
    const sink = (index: number) => ({
      onOutput() { throw new Error("Raw bytes required"); },
      onBytes(chunk: Uint8Array) { bytes[index].push(...chunk); written.resolve(); },
      onTruncated() {}, onEnded() {},
    });
    let detach = output.subscribeTabOutput("tab", sink(0), desktop, "pty");
    let detachBrowser = () => {};
    try {
      let controller = await connection.promise;
      const send = (target: ReadableStreamDefaultController<Uint8Array>, data: number[], offset: number) => {
        target.enqueue(new TextEncoder().encode(`data: ${JSON.stringify({ tab_id: "tab", pty_id: "pty", boot: "boot", b64: btoa(String.fromCharCode(...data)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, ""), offset })}\n\n`));
      };
      send(controller, [0x1b, 0xff, 0xe2], 3);
      await written.promise;
      written = pending<void>();
      send(controller, [0x1b, 0xff, 0xe2], 3);
      send(controller, [0xe2, 0x82, 0xac], 5);
      await written.promise;
      expect(bytes[0]).toEqual([0x1b, 0xff, 0xe2, 0x82, 0xac]);
      detach();
      expect(requests[0].signal.aborted).toBe(false);
      detach = output.subscribeTabOutput("tab", sink(0), desktop, "pty");
      expect(requests).toHaveLength(1);
      written = pending<void>();
      send(controller, [0x1b, 0xff, 0xe2, 0x82, 0xac], 5);
      send(controller, [6], 6);
      await written.promise;
      expect(bytes[0]).toEqual([0x1b, 0xff, 0xe2, 0x82, 0xac, 6]);
      detachBrowser = output.subscribeTabOutput("tab", sink(1), browser, "pty");
      expect(requests).toHaveLength(1);
      expect(bytes).toEqual([
        [0x1b, 0xff, 0xe2, 0x82, 0xac, 6], [0x1b, 0xff, 0xe2, 0x82, 0xac, 6],
      ]);
      connection = pending();
      token = "next-token";
      nonce = "next-binding";
      output.bindOutputStreamTransport(createTransportClient({
        origin: "http://replacement.test", token, headers: { "X-CodeTerm-Session-Binding-Nonce": nonce },
      }));
      controller = await connection.promise;
      expect(requests.slice(0, -1).every(({ signal }) => signal.aborted)).toBe(true);
      expect(requests[requests.length - 1]?.url.origin).toBe("http://replacement.test");
      written = pending<void>();
      send(controller, [7], 7);
      await written.promise;
      expect(bytes).toEqual([
        [0x1b, 0xff, 0xe2, 0x82, 0xac, 6, 7], [0x1b, 0xff, 0xe2, 0x82, 0xac, 6, 7],
      ]);
      expect(requests.every(({ url }) => url.pathname === "/api/output/stream")).toBe(true);
    } finally {
      detachBrowser(); detach(); output.resetOutputStream();
    }
  });
});


it("retains byte cursors across a capacity end and decodes once for multiple sinks", async () => {
  vi.resetModules();
  localStorage.clear();
  sessionStorage.setItem("codeterm-web-output-boot", "boot");
  const output = await import("./outputStream");
  let receive!: (name: string, data: unknown) => void;
  let end!: (value: import("./transportClient").StreamEnd) => void;
  const paths: string[] = [];
  output.bindOutputStreamTransport({ origin: () => "http://daemon.test", subscribe(path, handler, options) {
    paths.push(path); receive = handler; end = options!.onEnd!; return () => {};
  } } as import("./transportClient").TransportClient);
  const bytes: number[][] = [[], []];
  const errors = vi.fn();
  const ended = vi.fn();
  const stops = bytes.map((target) => output.subscribeTabOutput("t", {
    onOutput() { throw new Error("Expected bytes"); }, onBytes(chunk) { target.push(...chunk); },
    onEnded: ended, onTruncated() {}, onStreamError: errors,
  }, {}, "p"));
  const decode = vi.spyOn(globalThis, "atob");
  window.__outputStreamPerf = { frames: 0, bytes: 0, batches: 0, lastFrameAt: 0 };
  try {
    receive("message", { tab_id: "t", pty_id: "p", boot: "boot", offset: 3, b64: "AP_i" });
    expect(decode).toHaveBeenCalledTimes(1);
    expect(window.__outputStreamPerf.bytes).toBe(3);
    decode.mockRestore();
    end({ reason: "capacity", bytes: 2097153 });
    expect(errors).toHaveBeenCalledTimes(2);
    expect(paths).toHaveLength(1);
    output.pokeOutputStream();
    const since = new URL(paths[1], "http://daemon.test").searchParams.get("since")!;
    expect(JSON.parse(atob(since.replace(/-/g, "+").replace(/_/g, "/")))).toEqual({ boot: "boot", tabs: { t: 3 } });
    receive("message", { tab_id: "t", pty_id: "p", boot: "boot", offset: 3, b64: "AP_i" });
    receive("message", { tab_id: "t", pty_id: "p", boot: "boot", offset: 5, b64: "gqw" });
    expect(bytes).toEqual([[0, 255, 226, 130, 172], [0, 255, 226, 130, 172]]);
    receive("message", { tab_id: "t", ended: true });
    expect(ended).toHaveBeenCalledTimes(2);
  } finally {
    decode.mockRestore(); delete window.__outputStreamPerf;
    stops.forEach(stop => stop()); output.resetOutputStream();
  }
});


it.each(["local", "mesh"])("delivers captured terminal bytes through the multiplexed %s SSE to an xterm writer", async (route) => {
  vi.resetModules();
  localStorage.clear();
  sessionStorage.setItem("codeterm-web-output-boot", capture.boot);
  const output = await import("./outputStream");
  const { configureDaemonTransport } = await import("./daemonBootstrap");
  const { Terminal } = createRequire(import.meta.url)("@xterm/xterm") as typeof import("@xterm/xterm");
  const term = new Terminal({ cols: 87, rows: 37 });
  const written: number[] = [];
  const completed = pending<void>();
  const failures: Error[] = [];
  const fakeXterm = { write(data: string | Uint8Array, done?: () => void) {
    expect(data).toBeInstanceOf(Uint8Array);
    written.push(...data as Uint8Array);
    term.write(data, () => {
      done?.();
      if (written.length === capture.bytes) completed.resolve();
    });
  } };
  const frame = { tab_id: capture.tabId, pty_id: capture.ptyId, boot: capture.boot, offset: capture.bytes, b64: capture.b64 };
  let pushWire!: (bytes: Uint8Array) => void;
  const channelPaths: string[] = [];
  const fetch = vi.fn<typeof globalThis.fetch>(async (input, init) => {
    if (new URL(String(input)).pathname === "/api/stream-mux") {
      return new Response(new ReadableStream<Uint8Array>({ start(controller) { pushWire = (bytes) => controller.enqueue(bytes); } }));
    }
    const opened = JSON.parse(String(init?.body)) as { channel: string; path: string };
    channelPaths.push(opened.path);
    const wire = new TextEncoder().encode(`data: ${JSON.stringify({ channel: opened.channel, data: frame })}\n\n`);
    for (let index = 0; index < wire.length; index += 17) pushWire(wire.slice(index, index + 17));
    return Response.json({ ok: true });
  });
  vi.stubGlobal("fetch", fetch);
  const client = configureDaemonTransport({ origin: window.location.origin, token: "fixture-token" });
  output.bindOutputStreamTransport(client);
  const tabId = route === "local" ? capture.tabId : `mesh:928b06dd-1234-4567-890a-123456789abc/${capture.tabId}`;
  const stop = output.subscribeTabOutput(tabId, {
    onOutput() { throw new Error("Expected byte sink"); },
    onBytes(bytes) { writeTerminalOutput(fakeXterm, bytes); },
    onTruncated() {}, onEnded() {}, onStreamError(error) { failures.push(error); },
  }, fakeXterm, capture.ptyId);
  try {
    await completed.promise;
    expect(failures).toEqual([]);
    expect(written).toHaveLength(5956);
    const buffer = term.buffer.active;
    const text = Array.from({ length: buffer.length }, (_, index) => buffer.getLine(index)!.translateToString(true)).join("\n");
    expect(text).toContain("Merge");
    expect(createHash("sha256").update(Uint8Array.from(written)).digest("hex")).toBe(capture.sha256);
    expect(new URL(String(fetch.mock.calls[0][0])).pathname).toBe("/api/stream-mux");
    const requested = new URL(channelPaths[0], window.location.origin);
    expect(requested.pathname).toBe(route === "local" ? "/api/output/stream" : "/api/mesh/relay-stream");
    expect(atob(capture.b64.replace(/-/g, "+").replace(/_/g, "/")).length).toBe(capture.bytes);
  } finally { stop(); output.resetOutputStream(); term.dispose(); }
});
