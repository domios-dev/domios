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
    function Harness({ active }: { active: boolean }) {
      useTerminalInput({ ...options, terminalActive: active });
      return null;
    }
    try {
      await act(async () => root.render(<Harness active={false} />));
      await act(async () => emit("a"));
      await act(async () => emit("\x1b[0n"));
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
