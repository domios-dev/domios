import { afterEach, describe, expect, it, vi } from "vitest";
import { createTransportClient } from "./transportClient";
import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import capture from "./outputStream.capture.fixture.json";
import { writeTerminalOutput } from "../hooks/terminal/terminalOutputWrite";
import { OUTPUT_ACK_BYTES } from "../hooks/terminal/rendererOutputFlow";

function pending<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

function joinBytes(chunks: readonly Uint8Array[]): Uint8Array {
  const joined = new Uint8Array(chunks.reduce((total, chunk) => total + chunk.length, 0));
  let offset = 0;
  for (const chunk of chunks) {
    joined.set(chunk, offset);
    offset += chunk.length;
  }
  return joined;
}

interface OutputSseConnection {
  controller: ReadableStreamDefaultController<Uint8Array>;
  signal: AbortSignal;
}

function createOutputSseTransport() {
  const connections: OutputSseConnection[] = [];
  const waiters = new Map<number, Array<(connection: OutputSseConnection) => void>>();
  const client = createTransportClient({
    origin: "http://daemon.test",
    token: "fixture-token",
    fetch: async (_input, init) => {
      let controller!: ReadableStreamDefaultController<Uint8Array>;
      const signal = init?.signal as AbortSignal;
      const body = new ReadableStream<Uint8Array>({
        start(value) { controller = value; },
      });
      const connection = { controller, signal };
      const index = connections.push(connection) - 1;
      for (const resolve of waiters.get(index) ?? []) resolve(connection);
      waiters.delete(index);
      signal.addEventListener("abort", () => {
        try { controller.close(); } catch {}
      }, { once: true });
      return new Response(body, { headers: { "Content-Type": "text/event-stream" } });
    },
  });
  return {
    client,
    waitForConnection(index: number): Promise<OutputSseConnection> {
      const connection = connections[index];
      if (connection) return Promise.resolve(connection);
      return new Promise(resolve => {
        const pending = waiters.get(index) ?? [];
        pending.push(resolve);
        waiters.set(index, pending);
      });
    },
    send(connection: OutputSseConnection, frame: unknown): void {
      connection.controller.enqueue(new TextEncoder().encode(`data: ${JSON.stringify(frame)}\n\n`));
    },
  };
}

function outputFrame(tabId: string, ptyId: string, offset: number, bytes: Uint8Array, boot = "boot") {
  return {
    tab_id: tabId, pty_id: ptyId, boot, offset,
    b64: btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, ""),
  };
}

it("bounds no-progress capacity resumes and resets the budget after output progress", async () => {
  vi.useFakeTimers(); vi.resetModules(); sessionStorage.setItem("codeterm-web-output-boot", "boot");
  const output = await import("./outputStream");
  let end!: (end: import("./transportClient").StreamEnd) => void;
  let receive!: (name: string, frame: unknown) => void | Promise<void>;
  const paths: string[] = [], errors = vi.fn();
  output.bindOutputStreamTransport({ origin: () => "http://daemon.test", subscribe: (path, handler, options) => {
    paths.push(path); receive = handler; end = options!.onEnd!; return () => {};
  } } as import("./transportClient").TransportClient);
  const stop = output.subscribeTabOutput("own", { onOutput() {}, onTruncated() {}, onEnded() {}, onStreamError: errors }, {}, "pty");
  try {
    for (const delay of [125, 250]) {
      const count = paths.length;
      end({ reason: "capacity", bytes: 1 });
      await vi.advanceTimersByTimeAsync(delay - 1);
      expect(paths).toHaveLength(count);
      await vi.advanceTimersByTimeAsync(1);
      expect(paths).toHaveLength(count + 1);
    }
    await receive("message", { tab_id: "own", pty_id: "pty", boot: "boot", replay_complete: true, offset: 0 });
    const count = paths.length;
    end({ reason: "capacity", bytes: 1 });
    expect(paths).toHaveLength(count + 1);
    for (const delay of [125, 250]) {
      const count = paths.length;
      end({ reason: "capacity", bytes: 1 });
      await vi.advanceTimersByTimeAsync(delay);
      expect(paths).toHaveLength(count + 1);
    }
    const parked = paths.length;
    end({ reason: "capacity", bytes: 1 });
    expect(errors).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(20_000);
    expect(paths).toHaveLength(parked);
  } finally { stop(); output.resetOutputStream(); vi.useRealTimers(); }
});

it.each(["tab", "mesh:928b06dd-1234-4567-890a-123456789abc/tab"])("replays a reset %s ring even below the old byte cursor", async tabId => {
  vi.resetModules(); sessionStorage.setItem("codeterm-web-output-boot", "boot");
  const output = await import("./outputStream");
  let receive!: (name: string, frame: unknown) => void | Promise<void>;
  output.bindOutputStreamTransport({ origin: () => "http://daemon.test", subscribe: (_path, handler) => { receive = handler; return () => {}; } } as import("./transportClient").TransportClient);
  const written = vi.fn(), gaps = vi.fn();
  const stop = output.subscribeTabOutput(tabId, { onOutput: written, onTruncated: gaps, onEnded() {} }, {}, "own-pty");
  try {
    await receive("message", { tab_id: "tab", pty_id: "own-pty", boot: "boot", offset: 1000, b64: "WA" });
    await receive("message", { tab_id: "tab", pty_id: "foreign", boot: "boot", reset: true, truncated: true, from: 0 });
    expect(gaps).not.toHaveBeenCalled();
    await receive("message", { tab_id: "tab", pty_id: "own-pty", boot: "boot", reset: true, truncated: true, from: 0 });
    await receive("message", { tab_id: "tab", pty_id: "own-pty", boot: "boot", offset: 5, b64: "ZnJlc2g" });
    expect(gaps).toHaveBeenCalledWith(0);
    expect(written.mock.calls.map(([text]) => text)).toEqual(["X", "fresh"]);
  } finally { stop(); output.resetOutputStream(); }
});

it("lists mounted tabs for replay priority while omitting unmounted rings", async () => {
  vi.resetModules(); sessionStorage.setItem("codeterm-web-output-boot", "boot");
  const output = await import("./outputStream");
  const paths: string[] = [];
  let receive!: (name: string, frame: unknown) => void | Promise<void>;
  output.bindOutputStreamTransport({ origin: () => "http://daemon.test", subscribe: (path, handler) => { paths.push(path); receive = handler; return () => {}; } } as import("./transportClient").TransportClient);
  const sink = { onOutput() {}, onTruncated() {}, onEnded() {} };
  const stopOld = output.subscribeTabOutput("old", sink, {}, "old-pty");
  await receive("message", { tab_id: "old", pty_id: "old-pty", boot: "boot", offset: 1, b64: "WA" });
  stopOld();
  const stopNew = output.subscribeTabOutput("new", { ...sink }, {}, "new-pty");
  try {
    expect(paths).toHaveLength(1);
    output.restartOutputStream();
    expect(paths).toHaveLength(2);
    const since = new URL(paths[1], "http://daemon.test").searchParams.get("since")!;
    expect(JSON.parse(atob(since.replace(/-/g, "+").replace(/_/g, "/")))).toEqual({ boot: "boot", tabs: { new: { offset: 0, pty_id: "new-pty" } } });
  } finally { stopNew(); output.resetOutputStream(); }
});

afterEach(() => { vi.unstubAllGlobals(); });

describe("aggregate terminal attachment", () => {
  it("lets another exact PTY render while a write is held and ACKs ordered offsets only after completion", async () => {
    vi.resetModules(); localStorage.clear();
    sessionStorage.setItem("codeterm-web-output-boot", "boot");
    const output = await import("./outputStream");
    const transport = createOutputSseTransport();
    output.bindOutputStreamTransport(transport.client);
    const firstWrite = pending<void>(), secondWrite = pending<void>();
    const firstStarted = pending<void>(), secondStarted = pending<void>();
    const secondRendered = pending<void>(), otherRendered = pending<void>();
    const aWrites: number[][] = [], aRendered: number[] = [], bBytes: number[] = [], bRendered: number[] = [];
    const terminalA = {}, terminalB = {};
    const stopA = output.subscribeTabOutput("a", {
      onOutput() { throw new Error("Expected bytes"); },
      onBytes(bytes) {
        aWrites.push([...bytes]);
        if (aWrites.length === 1) { firstStarted.resolve(); return firstWrite.promise; }
        if (aWrites.length === 2) { secondStarted.resolve(); return secondWrite.promise; }
      },
      onRendered(offset) { aRendered.push(offset); if (offset === 3) secondRendered.resolve(); },
      onTruncated() {}, onEnded() {},
    }, terminalA, "pty-a");
    const stopB = output.subscribeTabOutput("b", {
      onOutput() { throw new Error("Expected bytes"); },
      onBytes(bytes) { bBytes.push(...bytes); },
      onRendered(offset) { bRendered.push(offset); otherRendered.resolve(); },
      onTruncated() {}, onEnded() {},
    }, terminalB, "pty-b");
    try {
      const connection = await transport.waitForConnection(0);
      transport.send(connection, outputFrame("a", "pty-a", 1, Uint8Array.of(65)));
      transport.send(connection, outputFrame("b", "pty-b", 1, Uint8Array.of(66)));
      await firstStarted.promise;
      await otherRendered.promise;
      expect(aRendered).toEqual([]);
      expect(bBytes).toEqual([66]);
      expect(bRendered).toEqual([1]);

      transport.send(connection, outputFrame("a", "pty-a", 3, Uint8Array.of(67, 68)));
      firstWrite.resolve();
      await secondStarted.promise;
      expect(aWrites).toEqual([[65], [67, 68]]);
      expect(aRendered).toEqual([1]);
      secondWrite.resolve();
      await secondRendered.promise;
      expect(aRendered).toEqual([1, 3]);
    } finally { stopB(); stopA(); output.resetOutputStream(); }
  });

  it("bounds one PTY's render backlog and catches it up in exact order after capacity returns", async () => {
    vi.resetModules(); localStorage.clear();
    sessionStorage.setItem("codeterm-web-output-boot", "boot");
    const output = await import("./outputStream");
    const transport = createOutputSseTransport();
    output.bindOutputStreamTransport(transport.client);
    const firstWrite = pending<void>(), firstStarted = pending<void>();
    const firstBRendered = pending<void>(), capacityReached = pending<void>();
    const lastAcknowledge = pending<void>(), secondBRendered = pending<void>();
    const aBytes: number[] = [], aRendered: number[] = [], aGaps: number[] = [], bRendered: number[] = [];
    let seenFrames = 0;
    window.__outputStreamPerf = {
      get frames() { return seenFrames; },
      set frames(value: number) { seenFrames = value; if (value === 18) capacityReached.resolve(); },
      bytes: 0, batches: 0, lastFrameAt: 0,
    };
    const stopA = output.subscribeTabOutput("a", {
      onOutput() { throw new Error("Expected bytes"); },
      onBytes(bytes) {
        aBytes.push(...bytes);
        if (aBytes.length === 1) { firstStarted.resolve(); return firstWrite.promise; }
      },
      onRendered(offset) { aRendered.push(offset); if (offset === 24) lastAcknowledge.resolve(); },
      onTruncated(from) { aGaps.push(from); }, onEnded() {},
    }, {}, "pty-a");
    const stopB = output.subscribeTabOutput("b", {
      onOutput() { throw new Error("Expected bytes"); },
      onBytes() {},
      onRendered(offset) {
        bRendered.push(offset);
        if (offset === 1) firstBRendered.resolve();
        if (offset === 2) secondBRendered.resolve();
      },
      onTruncated() {}, onEnded() {},
    }, {}, "pty-b");
    try {
      const connection = await transport.waitForConnection(0);
      transport.send(connection, outputFrame("a", "pty-a", 1, Uint8Array.of(1)));
      transport.send(connection, outputFrame("b", "pty-b", 1, Uint8Array.of(100)));
      await firstStarted.promise;
      await firstBRendered.promise;
      expect(bRendered).toEqual([1]);

      for (let offset = 2; offset <= 24; offset++) {
        transport.send(connection, outputFrame("a", "pty-a", offset, Uint8Array.of(offset)));
      }
      transport.send(connection, outputFrame("b", "pty-b", 2, Uint8Array.of(101)));
      await capacityReached.promise;
      expect(aBytes).toEqual([1]);
      expect(aRendered).toEqual([]);
      expect(bRendered).toEqual([1]);
      await secondBRendered.promise;
      expect(bRendered).toEqual([1, 2]);

      firstWrite.resolve();
      await lastAcknowledge.promise;
      expect(aBytes).toEqual(Array.from({ length: 24 }, (_, index) => index + 1));
      expect(aRendered).toEqual([...Array.from({ length: 16 }, (_, index) => index + 1), 24]);
      expect(aGaps).toEqual([]);
      expect(bRendered).toEqual([1, 2]);
    } finally {
      firstWrite.resolve();
      stopB(); stopA(); output.resetOutputStream(); delete window.__outputStreamPerf;
    }
  });

  it("reports the retained-tail gap after a non-owner stall exceeds retention and completes deferred controls in order", async () => {
    vi.resetModules(); localStorage.clear();
    sessionStorage.setItem("codeterm-web-output-boot", "boot");
    const output = await import("./outputStream");
    const transport = createOutputSseTransport();
    output.bindOutputStreamTransport(transport.client);
    const frameBytes = OUTPUT_ACK_BYTES;
    const frameCount = 80;
    const retentionBytes = 2 * 1024 * 1024;
    const totalBytes = frameBytes * frameCount;
    const queuedPrefixBytes = 8 * frameBytes;
    const gapOffset = totalBytes - retentionBytes;
    const outputBytes = new Uint8Array(totalBytes);
    for (let index = 0; index < outputBytes.length; index++) outputBytes[index] = index % 251;

    const firstWrite = pending<void>(), firstStarted = pending<void>();
    const catchupWrite = pending<void>(), catchupStarted = pending<void>();
    const replayWrite = pending<void>(), replayStarted = pending<void>(), endedReached = pending<void>();
    const aVisibleChunks: Uint8Array[] = [], aAckOffsets: number[] = [], aGaps: number[] = [];
    const bAckOffsets: number[] = [];
    const bAckWaiters = new Map<number, () => void>();
    let aWriteCount = 0, replayComplete = false, ended = false;
    const stopA = output.subscribeTabOutput("a", {
      onOutput() { throw new Error("Expected bytes"); },
      onBytes(bytes) {
        const renderedBytes = bytes.slice();
        if (aWriteCount++ === 0) { firstStarted.resolve(); return firstWrite.promise.then(() => aVisibleChunks.push(renderedBytes)); }
        if (bytes.length > retentionBytes / 2) { catchupStarted.resolve(); return catchupWrite.promise.then(() => aVisibleChunks.push(renderedBytes)); }
        aVisibleChunks.push(renderedBytes);
      },
      onRendered(offset) { aAckOffsets.push(offset); },
      onTruncated(from) { aGaps.push(from); },
      onReplayComplete() { replayComplete = true; replayStarted.resolve(); return replayWrite.promise; },
      onEnded() { ended = true; endedReached.resolve(); },
    }, {}, "pty-a");
    const stopB = output.subscribeTabOutput("b", {
      onOutput() { throw new Error("Expected bytes"); },
      onBytes() {},
      onRendered(offset) {
        bAckOffsets.push(offset);
        const acknowledge = bAckWaiters.get(offset);
        if (acknowledge) { bAckWaiters.delete(offset); acknowledge(); }
      },
      onTruncated() {}, onEnded() {},
    }, {}, "pty-b");
    const waitForOwnerAck = (offset: number) => new Promise<void>(resolve => bAckWaiters.set(offset, resolve));
    const sendProducerCredit = async (connection: OutputSseConnection, offset: number) => {
      const acknowledged = waitForOwnerAck(offset);
      const credit = new Uint8Array(frameBytes).fill(offset / frameBytes % 251);
      transport.send(connection, outputFrame("b", "pty-b", offset, credit));
      await acknowledged;
    };
    const sendAFrame = (connection: OutputSseConnection, index: number) => {
      const end = (index + 1) * frameBytes;
      transport.send(connection, outputFrame("a", "pty-a", end, outputBytes.subarray(end - frameBytes, end)));
    };
    try {
      const connection = await transport.waitForConnection(0);
      sendAFrame(connection, 0);
      await firstStarted.promise;
      // B is the separate producer-credit viewer; each real 32 KiB render ACK permits another A frame.
      for (let index = 0; index < frameCount - 1; index++) {
        await sendProducerCredit(connection, (index + 1) * frameBytes);
        sendAFrame(connection, index + 1);
      }
      transport.send(connection, { tab_id: "a", pty_id: "pty-a", boot: "boot", replay_complete: true, offset: totalBytes });
      transport.send(connection, { tab_id: "a", pty_id: "pty-a", boot: "boot", ended: true });
      await sendProducerCredit(connection, frameCount * frameBytes);

      expect(bAckOffsets).toEqual(Array.from({ length: frameCount }, (_, index) => (index + 1) * frameBytes));
      expect(aVisibleChunks).toEqual([]);
      expect(aAckOffsets).toEqual([]);
      expect(aGaps).toEqual([]);
      expect(replayComplete).toBe(false);
      expect(ended).toBe(false);

      firstWrite.resolve();
      await catchupStarted.promise;
      expect(joinBytes(aVisibleChunks)).toEqual(outputBytes.subarray(0, queuedPrefixBytes));
      expect(aAckOffsets).toEqual(Array.from({ length: queuedPrefixBytes / frameBytes }, (_, index) => (index + 1) * frameBytes));
      expect(aGaps).toEqual([gapOffset]);
      expect(replayComplete).toBe(false);
      expect(ended).toBe(false);

      catchupWrite.resolve();
      await replayStarted.promise;
      const expectedVisible = joinBytes([outputBytes.subarray(0, queuedPrefixBytes), outputBytes.subarray(gapOffset)]);
      expect(joinBytes(aVisibleChunks)).toEqual(expectedVisible);
      expect(aAckOffsets).toEqual([
        ...Array.from({ length: queuedPrefixBytes / frameBytes }, (_, index) => (index + 1) * frameBytes),
        totalBytes,
      ]);
      expect(replayComplete).toBe(true);
      expect(ended).toBe(false);

      replayWrite.resolve();
      await endedReached.promise;
      expect(aAckOffsets).toEqual([
        ...Array.from({ length: queuedPrefixBytes / frameBytes }, (_, index) => (index + 1) * frameBytes),
        totalBytes, totalBytes,
      ]);
      expect(aGaps).toEqual([gapOffset]);
      expect(ended).toBe(true);
    } finally {
      firstWrite.resolve(); catchupWrite.resolve(); replayWrite.resolve();
      stopB(); stopA(); output.resetOutputStream();
    }
  });

  it("does not let a detached held write advance the replacement terminal cursor", async () => {
    vi.resetModules(); localStorage.clear();
    sessionStorage.setItem("codeterm-web-output-boot", "boot");
    const output = await import("./outputStream");
    const transport = createOutputSseTransport();
    output.bindOutputStreamTransport(transport.client);
    const heldWrite = pending<void>(), heldStarted = pending<void>(), heldFinished = pending<void>();
    const terminal = {};
    const staleRendered = vi.fn(), replacementRendered = pending<void>();
    let replacementStop = () => {};
    const stale = output.subscribeTabOutput("a", {
      onOutput() { throw new Error("Expected bytes"); },
      onBytes() { heldStarted.resolve(); return heldWrite.promise.then(() => heldFinished.resolve()); },
      onRendered: staleRendered, onTruncated() {}, onEnded() {},
    }, terminal, "pty-a");
    try {
      const connection = await transport.waitForConnection(0);
      transport.send(connection, outputFrame("a", "pty-a", 1, Uint8Array.of(65)));
      await heldStarted.promise;
      stale();
      const replacementWrites: number[] = [];
      replacementStop = output.subscribeTabOutput("a", {
        onOutput() { throw new Error("Expected bytes"); },
        onBytes(bytes) { replacementWrites.push(...bytes); },
        onRendered(offset) { if (offset === 1) replacementRendered.resolve(); },
        onTruncated() {}, onEnded() {},
      }, terminal, "pty-a");
      await replacementRendered.promise;
      expect(replacementWrites).toEqual([65]);
      heldWrite.resolve();
      await heldFinished.promise;
      expect(staleRendered).not.toHaveBeenCalled();
    } finally { replacementStop(); stale(); output.resetOutputStream(); }
  });

  it("suppresses an old boot's held render ACK across the actual SSE parser", async () => {
    vi.resetModules(); localStorage.clear();
    sessionStorage.setItem("codeterm-web-output-boot", "boot");
    const output = await import("./outputStream");
    const transport = createOutputSseTransport();
    output.bindOutputStreamTransport(transport.client);
    const oldWrite = pending<void>(), oldStarted = pending<void>(), oldFinished = pending<void>();
    const newRendered = pending<void>();
    const oldBytes: number[] = [], newBytes: number[] = [], rendered: number[] = [], resets = vi.fn();
    let replayCompleteCalls = 0, endedCalls = 0;
    window.__outputStreamPerf = { frames: 0, bytes: 0, batches: 0, lastFrameAt: 0 };
    const stop = output.subscribeTabOutput("a", {
      onOutput() { throw new Error("Expected bytes"); },
      onBytes(bytes) {
        if (bytes[0] === 65) { oldBytes.push(...bytes); oldStarted.resolve(); return oldWrite.promise.then(() => oldFinished.resolve()); }
        newBytes.push(...bytes);
      },
      onRendered(offset) { rendered.push(offset); if (offset === 1 && resets.mock.calls.length > 0) newRendered.resolve(); },
      onReset: resets, onTruncated() {},
      onReplayComplete() { replayCompleteCalls++; },
      onEnded() { endedCalls++; },
    }, {}, "pty-a");
    try {
      const oldConnection = await transport.waitForConnection(0);
      transport.send(oldConnection, outputFrame("a", "pty-a", 1, Uint8Array.of(65)));
      await oldStarted.promise;
      for (let offset = 2; offset <= 17; offset++) {
        transport.send(oldConnection, outputFrame("a", "pty-a", offset, Uint8Array.of(offset)));
      }
      transport.send(oldConnection, { tab_id: "a", pty_id: "pty-a", boot: "boot", replay_complete: true, offset: 17 });
      transport.send(oldConnection, { tab_id: "a", pty_id: "pty-a", boot: "boot", ended: true });
      transport.send(oldConnection, { boot_changed: true, boot: "next-boot" });
      const nextConnection = await transport.waitForConnection(1);
      expect(resets).toHaveBeenCalledOnce();
      expect(window.__outputStreamPerf.frames).toBe(20);
      transport.send(nextConnection, outputFrame("a", "pty-a", 1, Uint8Array.of(66), "next-boot"));
      await newRendered.promise;
      expect(oldBytes).toEqual([65]);
      expect(newBytes).toEqual([66]);
      expect(replayCompleteCalls).toBe(0);
      expect(endedCalls).toBe(0);
      oldWrite.resolve();
      await oldFinished.promise;
      expect(rendered).toEqual([1]);
      expect(replayCompleteCalls).toBe(0);
      expect(endedCalls).toBe(0);
    } finally { oldWrite.resolve(); stop(); output.resetOutputStream(); delete window.__outputStreamPerf; }
  });

  it("acknowledges only completed xterm writes and disconnects even on capacity recovery", async () => {
    vi.resetModules();
    localStorage.clear();
    sessionStorage.setItem("codeterm-web-output-boot", "boot");
    const output = await import("./outputStream");
    let receive!: (name: string, frame: unknown) => void | Promise<void>;
    let ended!: (end: import("./transportClient").StreamEnd) => void;
    output.bindOutputStreamTransport({ origin: () => "http://daemon.test", subscribe: (_path, handler, options) => {
      receive = handler; ended = options!.onEnd!; return () => {};
    } } as import("./transportClient").TransportClient);
    const write = pending<void>();
    const rendered = vi.fn(), disconnected = vi.fn();
    const detach = output.subscribeTabOutput("tab", {
      onOutput: () => write.promise, onRendered: rendered, onDisconnected: disconnected, onTruncated() {}, onEnded() {},
    }, {}, "pty");
    try {
      const completed = receive("message", { tab_id: "tab", pty_id: "pty", boot: "boot", offset: 1, b64: "QQ" });
      expect(rendered).not.toHaveBeenCalled();
      await receive("message", { tab_id: "tab", pty_id: "pty", boot: "boot", offset: 1, b64: "QQ" });
      expect(rendered).not.toHaveBeenCalled();
      write.resolve();
      await completed;
      expect(rendered).toHaveBeenCalledExactlyOnceWith(1);
      ended({ reason: "capacity", bytes: 100 });
      expect(disconnected).toHaveBeenCalledOnce();
    } finally { detach(); output.resetOutputStream(); }
  });
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
        expect(JSON.parse(atob(since.replace(/-/g, "+").replace(/_/g, "/")))).toEqual({ boot: "boot", tabs: { tab: { offset: 42295516, pty_id: "pty" } } });
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
      expect(cursor()).toEqual({ boot: "boot", tabs: { tab: { offset: 100, pty_id: null } } });
      receive("message", { tab_id: "tab", truncated: true, from: 100 });
      output.resumeRemoteOutputStreams();
      expect(cursor()).toEqual({ boot: "boot", tabs: { tab: { offset: 100, pty_id: null } } });
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
    // A lagging client that made progress resumes at once from its delivered cursor.
    expect(errors).not.toHaveBeenCalled();
    expect(paths).toHaveLength(2);
    const since = new URL(paths[1], "http://daemon.test").searchParams.get("since")!;
    expect(JSON.parse(atob(since.replace(/-/g, "+").replace(/_/g, "/")))).toEqual({ boot: "boot", tabs: { t: { offset: 3, pty_id: "p" } } });
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
      return new Response(new ReadableStream<Uint8Array>({ start(controller) { pushWire = (bytes) => controller.enqueue(bytes); } }), { headers: { "Content-Type": "text/event-stream" } });
    }
    const opened = JSON.parse(String(init?.body)) as { channel: string; path: string };
    channelPaths.push(opened.path);
    const head = { channel: opened.channel, open: true, status: 200, headers: [["content-type", "text/event-stream"]] };
    const body = { channel: opened.channel, chunk: `data: ${JSON.stringify(frame)}

`, enc: "utf8" };
    const wire = new TextEncoder().encode([head, body].map((value) => `data: ${JSON.stringify(value)}

`).join(""));
    for (let index = 0; index < wire.length; index += 17) pushWire(wire.slice(index, index + 17));
    return Response.json({ accepted: true }, { status: 202 });
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
