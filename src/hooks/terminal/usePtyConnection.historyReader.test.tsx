import assert from "node:assert/strict";
import test from "node:test";
import { act, createElement, useRef } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { Terminal } from "@xterm/xterm";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { isTerminalReadingHistory } from "./terminalHistoryReaders";
import { usePtyConnection } from "./usePtyConnection";

function installDom(): void {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/history-reader" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "Event", { value: window.Event, configurable: true });
}

function fakeTerminal(active: { type: string; viewportY: number; baseY: number }) {
  return {
    buffer: { active },
    scrollToBottom() {},
    scrollToLine() {},
    write(_data: string, done?: () => void) {
      done?.();
    },
  } as unknown as Terminal;
}

test("a terminal hidden while scrolled into history is reported as a reader until shown or unmounted", async () => {
  installDom();
  const viewport = { type: "normal", viewportY: 120, baseY: 400 };
  const terminal = fakeTerminal(viewport);
  const store = createMuxStateStore(() => ({ tabs: {} }) as unknown as ExtendedState);
  function Probe({ renderEnabled }: { renderEnabled: boolean }) {
    const termRef = useRef<Terminal | null>(terminal);
    const connectedPtyRef = useRef<string | null>(null);
    usePtyConnection({
      termRef,
      connectedPtyRef,
      tabId: "tab-reader",
      ptyId: "pty-reader",
      termReady: false,
      onShellReady: () => {},
      onOutputChunk: () => {},
      ioEnabled: false,
      renderEnabled,
    });
    return null;
  }
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const render = (renderEnabled: boolean) =>
    act(async () => root.render(createElement(MuxStateStoreContext.Provider, { value: store }, createElement(Probe, { renderEnabled }))));

  await render(true);
  assert.equal(isTerminalReadingHistory("tab-reader"), false);
  await render(false);
  assert.equal(isTerminalReadingHistory("tab-reader"), true);
  await render(true);
  assert.equal(isTerminalReadingHistory("tab-reader"), false);

  viewport.viewportY = viewport.baseY;
  await render(false);
  assert.equal(isTerminalReadingHistory("tab-reader"), false, "a tab following the tail is free to unmount");

  viewport.viewportY = 10;
  await render(true);
  await render(false);
  assert.equal(isTerminalReadingHistory("tab-reader"), true);
  await act(async () => root.unmount());
  assert.equal(isTerminalReadingHistory("tab-reader"), false);
  host.remove();
});


test("a lazy PTY plan refusal refreshes locks once and does not respawn on rerender", async () => {
  installDom();
  const { __setInvokeForTest } = await import("../../lib/bridge");
  const terminal = fakeTerminal({ type: "normal", viewportY: 0, baseY: 0 });
  const store = createMuxStateStore(() => ({ tabs: {} }) as unknown as ExtendedState);
  let attempts = 0;
  let refreshes = 0;
  let spawned = 0;
  let ready = false;
  const refresh = () => { refreshes++; };
  window.addEventListener("codeterm-refresh-mux-state", refresh);
  __setInvokeForTest(async (command) => {
    assert.equal(command, "spawn_pty");
    attempts++;
    throw new Error(JSON.stringify({ error: { code: "plan_limit_locked", message: "opaque" } }));
  });
  function Probe() {
    const termRef = useRef<Terminal | null>(terminal);
    const connectedPtyRef = useRef<string | null>(null);
    usePtyConnection({
      termRef, connectedPtyRef, tabId: "locked-tab", termReady: true,
      onShellReady() {}, onOutputChunk() {},
      onPtySpawned() { spawned++; },
      onConnectionStateChange(value) { ready = value; },
    });
    return null;
  }
  const host = document.createElement("div");
  const root = createRoot(host);
  const render = () => act(async () => root.render(createElement(MuxStateStoreContext.Provider, { value: store }, createElement(Probe))));
  try {
    await render();
    assert.equal(refreshes, 1);
    await render();
    assert.equal(attempts, 1);
    assert.equal(spawned, 0);
    assert.equal(ready, false);
  } finally {
    await act(async () => root.unmount());
    __setInvokeForTest(null);
    window.removeEventListener("codeterm-refresh-mux-state", refresh);
  }
});
