import { act, createElement, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Terminal } from "@xterm/xterm";
import type { ExtendedState } from "../../store/multiplexer/tree";
import type { UsePtyConnectionReturn } from "./usePtyConnection";

const ORIGIN = window.location.origin;

interface Channel { channel: string; path: string }

// The daemon behind the real stream mux: one carrier SSE, channel opens by POST, frames pushed by the test.
function fakeDaemon() {
  const opened: Channel[] = [];
  let wire!: (frame: unknown) => void;
  const heads = new Set<string>();
  const fetch = vi.fn(async (input: RequestInfo | URL, init: RequestInit = {}) => {
    const url = new URL(String(input));
    const method = init.method ?? "GET";
    if (url.pathname === "/api/stream-mux") {
      const body = new ReadableStream<Uint8Array>({
        start(controller) {
          wire = (frame) => controller.enqueue(new TextEncoder().encode(`data: ${JSON.stringify(frame)}\n\n`));
        },
      });
      return new Response(body, { headers: { "Content-Type": "text/event-stream" } });
    }
    if (method === "POST" && url.pathname.endsWith("/channels")) {
      opened.push(JSON.parse(String(init.body)) as Channel);
      return Response.json({ accepted: true }, { status: 202 });
    }
    if (method === "DELETE") return Response.json({ ok: true, closed: true });
    return new Response("unexpected request", { status: 500 });
  });
  const outputChannels = () => opened.filter(({ path }) => new URL(path, ORIGIN).pathname === "/api/output/stream");
  const send = (channel: string, data: unknown) => {
    if (!heads.has(channel)) {
      heads.add(channel);
      wire({ channel, open: true, status: 200, headers: [["content-type", "text/event-stream"]] });
    }
    wire({ channel, chunk: "data: " + JSON.stringify(data) + "\n\n", enc: "utf8" });
  };
  const end = (channel: string, reason: unknown) => wire({ channel, end: reason });
  return { fetch, outputChannels, send, end };
}

function cursorOf(channel: Channel): { boot: string; tabs: Record<string, number> } {
  const since = new URL(channel.path, ORIGIN).searchParams.get("since")!;
  return JSON.parse(atob(since.replace(/-/g, "+").replace(/_/g, "/")));
}

interface Probe {
  frameReady: Map<string, string>;
  hook: Map<string, UsePtyConnectionReturn>;
  render: (tabs: Array<{ tabId: string; renderEnabled?: boolean }>) => Promise<void>;
}

async function boot() {
  vi.resetModules();
  localStorage.clear();
  sessionStorage.clear();
  sessionStorage.setItem("codeterm-web-output-boot", "boot");
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const daemon = fakeDaemon();
  vi.stubGlobal("fetch", daemon.fetch);
  const bootstrap = await import("../../lib/daemonBootstrap");
  const output = await import("../../lib/outputStream");
  const { usePtyConnection } = await import("./usePtyConnection");
  const { MuxStateStoreContext } = await import("../../store/multiplexer/context");
  const { createMuxStateStore } = await import("../../store/multiplexer/selectorStore");
  output.bindOutputStreamTransport(bootstrap.configureDaemonTransport({ origin: ORIGIN, token: "fixture" }));
  const store = createMuxStateStore(() => ({ tabs: {} }) as unknown as ExtendedState);
  const frameReady = new Map<string, string>();
  const hook = new Map<string, UsePtyConnectionReturn>();
  function Tab({ tabId, renderEnabled = true }: { tabId: string; renderEnabled?: boolean }) {
    const termRef = useRef({
      buffer: { active: { type: "normal", viewportY: 0, baseY: 0 } },
      scrollToBottom() {}, scrollToLine() {},
      // xterm acknowledges a parsed write asynchronously.
      write(_data: string | Uint8Array, done?: () => void) { if (done) queueMicrotask(done); },
      writeln() {},
    } as unknown as Terminal);
    const connectedPtyRef = useRef<string | null>(null);
    hook.set(tabId, usePtyConnection({
      termRef, connectedPtyRef, tabId, ptyId: `pty-${tabId}`, termReady: true, renderEnabled,
      onShellReady() {}, onOutputChunk() {},
      onFrameReady(ptyId) { frameReady.set(tabId, ptyId); },
    }));
    return null;
  }
  const root: Root = createRoot(document.createElement("div"));
  roots.push(root);
  const probe: Probe = {
    frameReady, hook,
    render: (tabs) => act(async () => root.render(createElement(MuxStateStoreContext.Provider, { value: store },
      tabs.map(tab => createElement(Tab, { key: tab.tabId, ...tab }))))),
  };
  return { daemon, output, probe };
}

const roots: Root[] = [];

afterEach(async () => {
  await act(async () => { for (const root of roots.splice(0)) root.unmount(); });
  (await import("../../lib/outputStream")).resetOutputStream();
  (await import("../../lib/streamMux")).__resetStreamMuxesForTest();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

const replayComplete = (tabId: string, offset = 0) => ({ tab_id: tabId, pty_id: `pty-${tabId}`, boot: "boot", offset, replay_complete: true });

describe("terminal frame readiness over the shared output channel", () => {
  it("a capacity end of the output channel reopens it from the delivered cursor, so a waiting terminal becomes frame-ready", async () => {
    const { daemon, probe } = await boot();
    await probe.render([{ tabId: "serve" }, { tabId: "hugo" }]);
    await vi.waitFor(() => expect(daemon.outputChannels()).toHaveLength(1));
    const first = daemon.outputChannels()[0].channel;
    daemon.send(first, { tab_id: "serve", pty_id: "pty-serve", boot: "boot", offset: 2, b64: "T0s" });
    daemon.send(first, replayComplete("serve", 2));
    await vi.waitFor(() => expect(probe.frameReady.get("serve")).toBe("pty-serve"));

    // The daemon overflowed the stream channel (carrier stalled behind a long task) before hugo's replay finished.
    daemon.end(first, { reason: "capacity", bytes: 512 * 1024 });

    await vi.waitFor(() => expect(daemon.outputChannels()).toHaveLength(2));
    const resumed = daemon.outputChannels()[1];
    expect(cursorOf(resumed)).toEqual({ boot: "boot", tabs: { serve: 2 } });
    daemon.send(resumed.channel, replayComplete("hugo"));
    await vi.waitFor(() => expect(probe.frameReady.get("hugo")).toBe("pty-hugo"));
    expect(probe.hook.get("hugo")!.readinessFailed).toBe(false);
  });

  it("activating a terminal revives an output channel that ended while the event stream stayed healthy", async () => {
    const { daemon, probe } = await boot();
    await probe.render([{ tabId: "hugo" }]);
    await vi.waitFor(() => expect(daemon.outputChannels()).toHaveLength(1));
    const first = daemon.outputChannels()[0].channel;
    daemon.send(first, { boot: "boot" });
    daemon.end(first, { reason: "eof" });
    await vi.waitFor(() => expect(probe.hook.get("hugo")!.readinessFailed).toBe(true));
    expect(daemon.outputChannels()).toHaveLength(1);

    // Chat/Terminal toggle: the terminal stops rendering, then activates again.
    await probe.render([{ tabId: "hugo", renderEnabled: false }]);
    await probe.render([{ tabId: "hugo", renderEnabled: true }]);

    await vi.waitFor(() => expect(daemon.outputChannels()).toHaveLength(2));
    daemon.send(daemon.outputChannels()[1].channel, replayComplete("hugo"));
    await vi.waitFor(() => expect(probe.frameReady.get("hugo")).toBe("pty-hugo"));
    expect(probe.hook.get("hugo")!.readinessFailed).toBe(false);
  });

  it("an output channel that keeps failing without progress parks, surfaces the failure, and Retry reopens it", async () => {
    const { daemon, probe } = await boot();
    await probe.render([{ tabId: "hugo" }]);
    await vi.waitFor(() => expect(daemon.outputChannels()).toHaveLength(1));
    daemon.end(daemon.outputChannels()[0].channel, { reason: "capacity", bytes: 1 });
    await vi.waitFor(() => expect(daemon.outputChannels()).toHaveLength(2));
    daemon.end(daemon.outputChannels()[1].channel, { reason: "capacity", bytes: 1 });
    await vi.waitFor(() => expect(probe.hook.get("hugo")!.readinessFailed).toBe(true));
    expect(daemon.outputChannels()).toHaveLength(2);

    await act(async () => probe.hook.get("hugo")!.retryConnection());

    await vi.waitFor(() => expect(daemon.outputChannels()).toHaveLength(3));
    expect(probe.hook.get("hugo")!.readinessFailed).toBe(false);
    daemon.send(daemon.outputChannels()[2].channel, replayComplete("hugo"));
    await vi.waitFor(() => expect(probe.frameReady.get("hugo")).toBe("pty-hugo"));
  });

  it("a live but silent output channel surfaces the failure at the bound and clears it when the frame arrives", async () => {
    const { daemon, probe } = await boot();
    const { TERMINAL_READINESS_FAILURE_MS } = await import("./usePtyConnection");
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    await probe.render([{ tabId: "hugo" }]);
    // vi.waitFor would advance the fake clock; the channel opens on promise turns alone.
    for (let turn = 0; turn < 100 && daemon.outputChannels().length === 0; turn++) await act(async () => { await Promise.resolve(); });
    expect(daemon.outputChannels()).toHaveLength(1);
    await act(async () => { await vi.advanceTimersByTimeAsync(TERMINAL_READINESS_FAILURE_MS - 1); });
    expect(probe.hook.get("hugo")!.readinessFailed).toBe(false);
    await act(async () => { await vi.advanceTimersByTimeAsync(1); });
    expect(probe.hook.get("hugo")!.readinessFailed).toBe(true);

    daemon.send(daemon.outputChannels()[0].channel, replayComplete("hugo"));
    await vi.waitFor(() => expect(probe.frameReady.get("hugo")).toBe("pty-hugo"));
    expect(probe.hook.get("hugo")!.readinessFailed).toBe(false);
  });
});
