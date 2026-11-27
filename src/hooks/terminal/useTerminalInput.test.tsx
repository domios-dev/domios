import "../../components/Terminal/ChatComposer.composition.harness";
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { Terminal } from "@xterm/xterm";
import { afterEach, describe, expect, finish, it } from "../../test/standaloneTest";
import { __setInvokeForTest } from "../../lib/bridge";
import { useTerminalInput } from "./useTerminalInput";

afterEach(() => __setInvokeForTest(null));

describe("terminal input ownership", () => {
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

  it("handles remote OSC 52 writes with user intent, refuses read queries, copies wrapped URLs, and pastes to remote PTY", async () => {
    const calls: Array<{ command: string; args?: Record<string, unknown> }> = [];
    const copied: string[] = [];
    const pasted: string[] = [];
    const originalNavigator = globalThis.navigator;
    Object.defineProperty(globalThis, "navigator", {
      configurable: true,
      value: {
        ...originalNavigator,
        platform: "Win32",
        clipboard: {
          writeText: async (text: string) => { copied.push(text); },
          readText: async () => "oauth-login-code-42",
          read: async () => [],
        },
      },
    });
    __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => {
      calls.push({ command, args });
      if (command === "read_clipboard_files") return [] as T;
      if (command === "read_clipboard_text") return "oauth-login-code-42" as T;
      return undefined as T;
    });

    let emitData: (data: string) => void = () => {};
    let keyHandler: ((e: KeyboardEvent) => boolean) | null = null;
    let osc52Handler: ((data: string) => boolean) | null = null;
    let selection = "  https://claude.ai/oauth/authorize?code=true&client_id=abc\r\n  &code_challenge=xyz&state=999";
    const term = {
      attachCustomKeyEventHandler(fn: (e: KeyboardEvent) => boolean) { keyHandler = fn; },
      onData(handler: (data: string) => void) {
        emitData = handler;
        return { dispose() {} };
      },
      getSelection: () => selection,
      hasSelection: () => selection.length > 0,
      clearSelection: () => { selection = ""; },
      paste: (data: string) => {
        pasted.push(data);
        emitData(data);
      },
      focus() {},
      parser: {
        registerOscHandler(id: number, handler: (data: string) => boolean) {
          if (id === 52) osc52Handler = handler;
          return { dispose() {} };
        },
      },
    } as unknown as Terminal;

    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    let exposed: ReturnType<typeof useTerminalInput> | null = null;
    function Harness({ replayingHistory }: { replayingHistory: boolean }) {
      exposed = useTerminalInput({
        tabId: "mesh:peer/remote-claude",
        termRef: { current: term },
        connectedPtyRef: { current: "mesh:peer/remote-claude" },
        containerRef: { current: host },
        termReady: true,
        terminalActive: true,
        isRemote: true,
        replayingHistory,
        onTabKey: () => false,
        onNonTabKey() {},
        onInputData() {},
      });
      return null;
    }

    try {
      await act(async () => root.render(<Harness replayingHistory={true} />));
      const encoded = Buffer.from("https://claude.ai/oauth/authorize?state=osc52", "utf8").toString("base64");
      // Blocked during history replay.
      expect(osc52Handler!(`c;${encoded}`)).toBe(true);
      expect(copied).toEqual([]);

      await act(async () => root.render(<Harness replayingHistory={false} />));
      // Blocked without prior user intent.
      expect(osc52Handler!(`c;${encoded}`)).toBe(true);
      expect(copied).toEqual([]);

      // User presses 'c' in Claude login -> remote PTY emits OSC 52 -> viewer clipboard updated.
      await act(async () => emitData("c"));
      expect(osc52Handler!(`c;${encoded}`)).toBe(true);
      await act(async () => Promise.resolve());
      expect(copied).toEqual(["https://claude.ai/oauth/authorize?state=osc52"]);

      // OSC 52 read query never sends clipboard contents to the remote PTY.
      const writesBeforeQuery = calls.length;
      expect(osc52Handler!("c;?")).toBe(true);
      expect(calls.length).toBe(writesBeforeQuery);

      // Copy selection unwraps the multi-line OAuth URL into one clean URL.
      await act(async () => exposed!.copySelection());
      expect(copied[copied.length - 1]).toBe(
        "https://claude.ai/oauth/authorize?code=true&client_id=abc&code_challenge=xyz&state=999",
      );

      // Ctrl+Shift+C copies selection without sending SIGINT.
      const shiftCopyEvent = new KeyboardEvent("keydown", {
        code: "KeyC", key: "C", ctrlKey: true, shiftKey: true, cancelable: true,
      });
      expect(keyHandler!(shiftCopyEvent)).toBe(false);
      await act(async () => Promise.resolve());
      expect(copied[copied.length - 1]).toBe(
        "https://claude.ai/oauth/authorize?code=true&client_id=abc&code_challenge=xyz&state=999",
      );

      // Paste routes viewer clipboard text into the remote PTY via mesh_remote_tab_action.
      await act(async () => exposed!.pasteFromClipboard());
      expect(pasted).toEqual(["oauth-login-code-42"]);
      expect(calls[calls.length - 1]).toEqual({
        command: "mesh_remote_tab_action",
        args: {
          ordinaryTabId: "mesh:peer/remote-claude",
          peerId: "peer",
          method: "POST",
          suffix: "/pty/write",
          body: { data: "oauth-login-code-42", raw: true },
        },
      });
    } finally {
      Object.defineProperty(globalThis, "navigator", { configurable: true, value: originalNavigator });
      await act(async () => root.unmount());
      host.remove();
    }
  });
});

finish();
