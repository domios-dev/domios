import assert from "node:assert/strict";
import { act, createElement, useLayoutEffect, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type { Terminal } from "@xterm/xterm";
import type { FitAddon } from "@xterm/addon-fit";
import { afterEach, beforeEach, describe, it, vi } from "vitest";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import type { ResizeOwner } from "../../lib/resizeOwner";
import { TERMINAL_VIEWER_RECONNECTED_EVENT } from "../../lib/terminalViewerEvents";
import { useTerminalActivationResize } from "../../hooks/terminal/useTerminalActivationResize";
import { useTerminalResize } from "../../hooks/terminal/useTerminalResize";

interface FakeTerminal {
  cols: number;
  rows: number;
  resizeListeners: Set<(size: { cols: number; rows: number }) => void>;
  onResize: (listener: (size: { cols: number; rows: number }) => void) => { dispose: () => void };
  setSize: (cols: number, rows: number) => void;
  focus: () => void;
  refresh: () => void;
}

const testWindow = new Window({ url: "http://localhost/" });
Object.assign(globalThis, {
  window: testWindow,
  document: testWindow.document,
  Element: testWindow.Element,
  HTMLElement: testWindow.HTMLElement,
  Node: testWindow.Node,
  SVGElement: testWindow.SVGElement,
  Event: testWindow.Event,
  CustomEvent: testWindow.CustomEvent,
  IS_REACT_ACT_ENVIRONMENT: true,
});

let frameId = 0;
const frames = new Map<number, FrameRequestCallback>();
Object.assign(globalThis, {
  requestAnimationFrame: (callback: FrameRequestCallback) => {
    const id = ++frameId;
    frames.set(id, callback);
    return id;
  },
  cancelAnimationFrame: (id: number) => frames.delete(id),
});
Object.assign(globalThis, {
  requestAnimationFrame: globalThis.requestAnimationFrame,
  cancelAnimationFrame: globalThis.cancelAnimationFrame,
  ResizeObserver: class {
    observe() {}
    disconnect() {}
  },
});
Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });

const resizeClientId = "web:viewer";
const baseOwner: ResizeOwner = {
  tab_id: "tab",
  pty_id: "pty",
  client_id: resizeClientId,
  cols: 80,
  rows: 24,
  updated_at: 1,
};

let root: Root | null = null;
let host: HTMLElement | null = null;
let terminal: FakeTerminal;
let targetSize = { cols: 160, rows: 48 };
let currentPtySize = { cols: 80, rows: 24 };
let claims: Array<{ args: Record<string, unknown>; owner: ResizeOwner }> = [];
let resizes: Array<Record<string, unknown>> = [];
let holdClaims = false;
let eventHandlers = new Map<string, (event: { payload: unknown }) => void>();
let activateViewer: (() => void) | null = null;

function createTerminal(): FakeTerminal {
  const value = {
    cols: 80,
    rows: 24,
    resizeListeners: new Set<(size: { cols: number; rows: number }) => void>(),
    onResize(listener: (size: { cols: number; rows: number }) => void) {
      value.resizeListeners.add(listener);
      return { dispose: () => value.resizeListeners.delete(listener) };
    },
    setSize(cols: number, rows: number) {
      if (value.cols === cols && value.rows === rows) return;
      value.cols = cols;
      value.rows = rows;
      for (const listener of value.resizeListeners) listener({ cols, rows });
    },
    focus() {},
    refresh() {},
  };
  return value;
}

function Harness({ active }: { active: boolean }) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef(terminal as unknown as Terminal | null);
  const fitAddonRef = useRef({ fit: () => terminal.setSize(targetSize.cols, targetSize.rows) } as unknown as FitAddon | null);
  const connectedPtyRef = useRef<string | null>("pty");
  const resize = useTerminalResize({
    containerRef,
    termRef,
    fitAddonRef,
    connectedPtyRef,
    tabId: "tab",
    ptyId: "pty",
    termReady: true,
    isFocused: active,
    resizeEnabled: active,
    resizeClientId,
  });
  activateViewer = useTerminalActivationResize({
    active,
    isFocused: active,
    termReady: true,
    tabId: "tab",
    ptyId: "pty",
    resizeClientId,
    containerRef,
    termRef,
    fitAddonRef,
    connectedPtyRef,
    setResizeOwner: resize.setResizeOwner,
    loseResizeOwnership: resize.loseResizeOwnership,
    hasResizeOwnership: resize.hasResizeOwnership,
    refitAndCommitResize: resize.refitAndCommitResize,
  });
  useLayoutEffect(() => {
    if (!containerRef.current) return;
    Object.defineProperty(containerRef.current, "clientWidth", { configurable: true, value: 1280 });
    Object.defineProperty(containerRef.current, "clientHeight", { configurable: true, value: 720 });
  }, []);
  return createElement("div", { ref: containerRef });
}

async function flushFrames() {
  await act(async () => {
    for (let count = 0; count < 30; count++) {
      const first = frames.entries().next().value as [number, FrameRequestCallback] | undefined;
      if (!first) {
        await Promise.resolve();
        if (frames.size === 0) break;
        continue;
      }
      frames.delete(first[0]);
      first[1](0);
      await Promise.resolve();
      await Promise.resolve();
    }
  });
}

async function mount(active: boolean) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(createElement(Harness, { active })));
  await flushFrames();
}

async function renderActive(active: boolean) {
  await act(async () => root!.render(createElement(Harness, { active })));
  await flushFrames();
}

async function cleanup() {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = null;
  host = null;
  frames.clear();
  eventHandlers.clear();
  activateViewer = null;
  __setInvokeForTest(null);
  __setListenForTest(null);
  vi.useRealTimers();
}

function configureBridge() {
  __setInvokeForTest(async <T,>(command: string, args = {}) => {
    if (command === "claim_resize_owner") {
      const owner = { ...baseOwner, cols: Number(args.cols), rows: Number(args.rows), updated_at: claims.length + 2 };
      claims.push({ args, owner });
      if (holdClaims) return new Promise<T>(() => {});
      return { owner, current_size: { ...currentPtySize } } as T;
    }
    if (command === "resize_pty") {
      resizes.push(args);
      currentPtySize = { cols: Number(args.cols), rows: Number(args.rows) };
      return { ok: true } as T;
    }
    throw new Error(`unexpected invoke: ${command}`);
  });
  __setListenForTest(async (event, handler) => {
    eventHandlers.set(event, handler as (event: { payload: unknown }) => void);
    return () => eventHandlers.delete(event);
  });
}

beforeEach(() => {
  terminal = createTerminal();
  targetSize = { cols: 160, rows: 48 };
  currentPtySize = { cols: 80, rows: 24 };
  claims = [];
  resizes = [];
  holdClaims = false;
  eventHandlers = new Map();
  activateViewer = null;
  Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
  configureBridge();
});

afterEach(cleanup);

describe("terminal viewer activation resize", () => {
  it("force-claims once and sends one fitted resize after another client left a narrow PTY", async () => {
    await mount(true);
    assert.equal(claims.length, 1);
    assert.equal(claims[0]?.args.force, true);
    assert.equal(claims[0]?.args.cols, 160);
    assert.equal(claims[0]?.args.rows, 48);
    assert.deepEqual(resizes.map(({ cols, rows }) => ({ cols, rows })), [{ cols: 160, rows: 48 }]);
  });

  it("does not resize when the current PTY already matches the fitted screen", async () => {
    currentPtySize = { cols: 160, rows: 48 };
    await mount(true);
    assert.equal(claims.length, 1);
    assert.equal(resizes.length, 0);
  });

  it("does not claim from a hidden prewarm mount and claims when that view becomes active", async () => {
    await mount(false);
    assert.equal(claims.length, 0);
    assert.equal(eventHandlers.has("pty-resize-owner-changed"), false);
    await renderActive(true);
    assert.equal(claims.length, 1);
    assert.equal(resizes.length, 1);
    assert.equal(eventHandlers.has("pty-resize-owner-changed"), true);
  });

  it("reclaims and fits once when document visibility returns", async () => {
    await mount(true);
    assert.equal(resizes.length, 1);

    Object.defineProperty(document, "visibilityState", { configurable: true, value: "hidden" });
    await act(async () => document.dispatchEvent(new Event("visibilitychange")));
    currentPtySize = { cols: 90, rows: 30 };
    Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
    await act(async () => document.dispatchEvent(new Event("visibilitychange")));
    await flushFrames();
    assert.equal(claims.length, 2);
    assert.equal(resizes.length, 2);
  });

  it("refits on reconnect only while this viewer still owns the resize", async () => {
    await mount(true);
    assert.equal(claims.length, 1);
    assert.equal(resizes.length, 1);

    targetSize = { cols: 140, rows: 42 };
    await act(async () => window.dispatchEvent(new Event(TERMINAL_VIEWER_RECONNECTED_EVENT)));
    await flushFrames();
    assert.equal(claims.length, 1);
    assert.deepEqual(resizes.map(({ cols, rows }) => ({ cols, rows })), [
      { cols: 160, rows: 48 }, { cols: 140, rows: 42 },
    ]);

    const ownerChanged = eventHandlers.get("pty-resize-owner-changed");
    assert.ok(ownerChanged);
    await act(async () => ownerChanged({ payload: { ...baseOwner, client_id: "web:phone", cols: 80, rows: 24 } }));
    targetSize = { cols: 120, rows: 40 };
    await act(async () => window.dispatchEvent(new Event(TERMINAL_VIEWER_RECONNECTED_EVENT)));
    await flushFrames();
    assert.equal(claims.length, 1);
    assert.equal(resizes.length, 2);
  });

  it("keeps resize ownership across window blur", async () => {
    await mount(true);
    assert.equal(resizes.length, 1);
    await act(async () => window.dispatchEvent(new Event("blur")));
    targetSize = { cols: 120, rows: 40 };
    await act(async () => window.dispatchEvent(new Event("codeterm-tab-resize")));
    await flushFrames();
    assert.equal(claims.length, 1);
    assert.deepEqual(resizes.map(({ cols, rows }) => ({ cols, rows })), [
      { cols: 160, rows: 48 }, { cols: 120, rows: 40 },
    ]);
  });

  it("stops committing when a different resize owner event arrives", async () => {
    await mount(true);
    const ownerChanged = eventHandlers.get("pty-resize-owner-changed");
    assert.ok(ownerChanged);
    await act(async () => ownerChanged({ payload: { ...baseOwner, client_id: "web:phone", cols: 80, rows: 24 } }));
    targetSize = { cols: 120, rows: 40 };
    await act(async () => window.dispatchEvent(new Event("codeterm-tab-resize")));
    await flushFrames();
    assert.equal(resizes.length, 1);
  });

  it("explicit activation callback force-claims the active terminal", async () => {
    currentPtySize = { cols: 160, rows: 48 };
    await mount(true);
    assert.equal(typeof activateViewer, "function");
    await act(async () => activateViewer?.());
    await flushFrames();
    assert.equal(claims.length, 2);
    assert.equal(claims[1]?.args.force, true);
    assert.equal(resizes.length, 0);
  });

  it("releases a hung claim after its failure deadline", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    holdClaims = true;
    await mount(true);
    assert.equal(claims.length, 1);

    await act(async () => vi.advanceTimersByTimeAsync(10_000));
    holdClaims = false;
    await act(async () => activateViewer?.());
    await flushFrames();
    assert.equal(claims.length, 2);
  });
});
