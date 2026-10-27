import "../../components/Terminal/ChatComposer.composition.harness";
// WHAT: Terminal input ownership and helper undo reset wiring.
// WHY: Submits must retire helper history without changing PTY payloads or IME drafts.
// ORACLE: Exact bridge writes and same-node detach/reattach only after eligible CR events.
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { Terminal } from "@xterm/xterm";
import { afterEach, describe, expect, finish, it } from "../../test/standaloneTest";
import { __setInvokeForTest } from "../../lib/bridge";
import { useTerminalInput } from "./useTerminalInput";

afterEach(() => __setInvokeForTest(null));

describe("terminal input ownership", () => {
  it("resets the same empty helper after CR and preserves focus, listeners, drafts and teardown", async () => {
    __setInvokeForTest(async <T,>() => undefined as T);
    let emit: (data: string) => void = () => { throw new Error("onData not registered"); };
    const host = document.createElement("div");
    const textarea = document.createElement("textarea");
    const sibling = document.createElement("span");
    const foreign = document.createElement("textarea");
    foreign.value = "composer draft";
    host.append(textarea, sibling, foreign);
    document.body.append(host);
    let removals = 0;
    const remove = host.removeChild.bind(host);
    host.removeChild = <T extends Node,>(node: T): T => { removals++; return remove(node); };
    let inputs = 0;
    textarea.addEventListener("input", () => inputs++);
    const term = {
      textarea,
      attachCustomKeyEventHandler() {},
      onData(handler: (data: string) => void) { emit = handler; return { dispose() {} }; },
    } as unknown as Terminal;
    const options = {
      tabId: "undo-fixture", termRef: { current: term }, connectedPtyRef: { current: null },
      containerRef: { current: host }, termReady: true, terminalActive: true,
      onTabKey: () => false, onNonTabKey() {}, onInputData() {},
    };
    const rootHost = document.createElement("div");
    const root = createRoot(rootHost);
    function Harness() { useTerminalInput(options); return null; }
    try {
      await act(async () => root.render(<Harness />));
      textarea.focus();
      emit("\r");
      expect(removals).toBe(0);
      await Promise.resolve();
      expect(removals).toBe(1);
      expect(host.firstChild).toBe(textarea);
      expect(textarea.nextSibling).toBe(sibling);
      expect(document.activeElement).toBe(textarea);
      textarea.dispatchEvent(new Event("input"));
      expect(inputs).toBe(1);
      expect(foreign.value).toBe("composer draft");

      textarea.value = "new draft";
      emit("\r");
      await Promise.resolve();
      expect(removals).toBe(1);
      expect(textarea.value).toBe("new draft");
      textarea.dispatchEvent(new Event("compositionstart"));
      textarea.value = "";
      emit("\r");
      textarea.dispatchEvent(new Event("compositionend"));
      await Promise.resolve();
      expect(removals).toBe(1);
      emit("\r");
      await act(async () => root.unmount());
      expect(removals).toBe(1);
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });

  it("keeps hidden Chat terminal onData from writing and routes visible terminal input", async () => {
    const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
    __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => {
      calls.push({ command, args });
      return undefined as T;
    });
    let emit: (data: string) => void = () => { throw new Error("onData not registered"); };
    const term = {
      attachCustomKeyEventHandler() {},
      onData(handler: (data: string) => void) {
        emit = handler;
        return { dispose() {} };
      },
    } as unknown as Terminal;
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    const options = {
      tabId: "mesh:peer/remote-input-tab",
      termRef: { current: term },
      connectedPtyRef: { current: "foreign-pty" },
      containerRef: { current: host },
      termReady: true,
      onTabKey: () => false,
      onNonTabKey() {},
      onInputData() {},
    };
    function Harness({ active, readOnly = false }: { active: boolean; readOnly?: boolean }) {
      useTerminalInput({ ...options, terminalActive: active, readOnly });
      return null;
    }
    try {
      await act(async () => root.render(<Harness active={false} />));
      await act(async () => emit("a"));
      await act(async () => emit("\x1b[0n"));
      expect(calls).toEqual([]);
      await act(async () => root.render(<Harness active readOnly />));
      await act(async () => emit("blocked"));
      await act(async () => emit("\x1b"));
      host.dispatchEvent(new Event("paste", { bubbles: true, cancelable: true }));
      expect(calls).toEqual([]);
      await act(async () => root.render(<Harness active />));
      await act(async () => emit("b"));
      expect(calls).toEqual([{
        command: "mesh_remote_tab_action",
        args: {
          ordinaryTabId: "mesh:peer/remote-input-tab",
          peerId: "peer",
          method: "POST",
          suffix: "/pty/write",
          body: { data: "b", raw: true },
        },
      }]);
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});

finish();
