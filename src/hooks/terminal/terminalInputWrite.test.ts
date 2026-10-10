import "../../components/Terminal/ChatComposer.composition.harness";
import { afterEach, describe, expect, finish, it } from "../../test/standaloneTest";
import { __setInvokeForTest } from "../../lib/bridge";
import { terminalInputTarget, TERMINAL_INPUT_WRITE_FAILED_EVENT, TERMINAL_INPUT_WRITE_SUCCEEDED_EVENT, writeTerminalInput } from "./terminalInputWrite";

type Call = { command: string; args?: Record<string, unknown> };

function captureWrites(write: (call: Call) => Promise<unknown> = async () => undefined): Call[] {
  const calls: Call[] = [];
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => {
    const call = { command, args };
    calls.push(call);
    return await write(call) as T;
  });
  return calls;
}

afterEach(() => __setInvokeForTest(null));

describe("terminal input transport", () => {
  it("routes a remote tab by composite identity despite a bare foreign PTY id", async () => {
    const calls = captureWrites();
    await writeTerminalInput("mesh:peer-a/shared-tab", "foreign-pty", "a");
    await writeTerminalInput("mesh:peer-a/shared-tab", "foreign-pty", "paste", false);
    expect(calls).toEqual([
      {
        command: "mesh_remote_tab_action",
        args: {
          ordinaryTabId: "mesh:peer-a/shared-tab", peerId: "peer-a",
          method: "POST", suffix: "/pty/write", body: { data: "a", raw: true },
        },
      },
      {
        command: "mesh_remote_tab_action",
        args: {
          ordinaryTabId: "mesh:peer-a/shared-tab", peerId: "peer-a",
          method: "POST", suffix: "/pty/write", body: { data: "paste", raw: false },
        },
      },
    ]);
  });

  it("preserves local PTY writes and raw flags", async () => {
    const calls = captureWrites();
    await writeTerminalInput("local-tab", "local-pty", "key");
    await writeTerminalInput("local-tab", "local-pty", "raw", true);
    await writeTerminalInput("local-tab", "local-pty", "paste", false);
    expect(calls).toEqual([
      { command: "write_pty_string", args: { ptyId: "local-pty", data: "key" } },
      { command: "write_pty_string", args: { ptyId: "local-pty", data: "raw", raw: true } },
      { command: "write_pty_string", args: { ptyId: "local-pty", data: "paste", raw: false } },
    ]);
  });

  it("keeps colliding local and remote PTYs on independent ordered routes", async () => {
    let release!: () => void;
    const blocked = new Promise<void>((resolve) => { release = resolve; });
    const calls = captureWrites(async (call) => {
      if (call.args?.ordinaryTabId === "mesh:peer-a/collision-tab") await blocked;
    });
    const first = writeTerminalInput("mesh:peer-a/collision-tab", "same-pty", "a");
    const next = writeTerminalInput("mesh:peer-a/collision-tab", "same-pty", "b");
    const last = writeTerminalInput("mesh:peer-a/collision-tab", "same-pty", "c");
    await writeTerminalInput("mesh:peer-b/collision-tab", "same-pty", "other");
    await writeTerminalInput("collision-tab", "same-pty", "local");
    expect(calls.map((call) => call.args?.ordinaryTabId ?? call.args?.ptyId))
      .toEqual(["mesh:peer-a/collision-tab", "mesh:peer-b/collision-tab", "same-pty"]);
    release();
    await Promise.all([first, next, last]);
    expect(calls[3]).toEqual({
      command: "mesh_remote_tab_action",
      args: {
        ordinaryTabId: "mesh:peer-a/collision-tab", peerId: "peer-a",
        method: "POST", suffix: "/pty/write", body: { data: "bc", raw: true },
      },
    });
  });

  it("notifies only routed failures without replaying input, and rearms after success", async () => {
    let fail = false;
    const calls = captureWrites(async () => { if (fail) throw new Error("transport failed"); });
    const failures: string[] = [];
    const listener = (event: Event) => {
      failures.push((event as CustomEvent<{ targetId: string }>).detail.targetId);
    };
    window.addEventListener(TERMINAL_INPUT_WRITE_FAILED_EVENT, listener);
    try {
      await writeTerminalInput("mesh:peer-a/failing-tab", "same-failing-pty", "ok");
      expect(failures).toEqual([]);
      fail = true;
      await writeTerminalInput("mesh:peer-a/failing-tab", "same-failing-pty", "failed");
      await writeTerminalInput("mesh:peer-a/failing-tab", "same-failing-pty", "later");
      expect(failures).toEqual(["mesh:peer-a/failing-tab"]);
      await writeTerminalInput("local-failing-tab", "same-failing-pty", "local-failed");
      expect(failures).toEqual(["mesh:peer-a/failing-tab", "same-failing-pty"]);
      fail = false;
      await writeTerminalInput("mesh:peer-a/failing-tab", "same-failing-pty", "recovered");
      fail = true;
      await writeTerminalInput("mesh:peer-a/failing-tab", "same-failing-pty", "new-failure");
      expect(failures).toEqual(["mesh:peer-a/failing-tab", "same-failing-pty", "mesh:peer-a/failing-tab"]);
      expect(calls.map((call) => call.args?.body ?? call.args?.data)).toEqual([
        { data: "ok", raw: true }, { data: "failed", raw: true }, { data: "later", raw: true },
        "local-failed", { data: "recovered", raw: true }, { data: "new-failure", raw: true },
      ]);
    } finally {
      window.removeEventListener(TERMINAL_INPUT_WRITE_FAILED_EVENT, listener);
    }
  });

  it("publishes success for the routed target only after a nonempty write succeeds", async () => {
    captureWrites();
    const successes: string[] = [];
    const listener = (event: Event) => successes.push((event as CustomEvent<{ targetId: string }>).detail.targetId);
    window.addEventListener(TERMINAL_INPUT_WRITE_SUCCEEDED_EVENT, listener);
    try {
      await writeTerminalInput("mesh:success/tab", "pty", "");
      expect(successes).toEqual([]);
      await writeTerminalInput("mesh:success/tab", "pty", "key");
      expect(successes).toEqual(["mesh:success/tab"]);
    } finally {
      window.removeEventListener(TERMINAL_INPUT_WRITE_SUCCEEDED_EVENT, listener);
    }
  });

  it("rejects malformed remote ownership before local transport and skips empty input", async () => {
    const calls = captureWrites();
    expect(() => writeTerminalInput("mesh:broken", "foreign-pty", "x"))
      .toThrow("local input fallback is forbidden");
    await writeTerminalInput("mesh:peer/empty-tab", "foreign-pty", "");
    expect(calls).toEqual([]);
    expect(terminalInputTarget("mesh:peer/tab", "foreign-pty")).toBe("mesh:peer/tab");
    expect(terminalInputTarget("local-tab", "local-pty")).toBe("local-pty");
  });
});

finish();
