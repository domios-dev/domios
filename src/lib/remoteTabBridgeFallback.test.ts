import { afterEach, describe, expect, finish, it } from "../test/standaloneTest";
import { __setInvokeForTest, invoke } from "./bridge";
import { subscribeTabOutput } from "./outputStream";

afterEach(() => __setInvokeForTest(null));

describe("bridge remote-shaped target guard", () => {
  it.each(["mux_set_focused_tab", "mux_close_tab", "mux_rename_tab", "get_tab_text", "write_pty_string"])(
    "rejects malformed mesh target before %s reaches the local bridge",
    async (command) => {
      const calls: unknown[][] = [];
      const local = async <T,>(...args: unknown[]): Promise<T> => {
        calls.push(args);
        return undefined as T;
      };
      __setInvokeForTest(local);
      await expect(invoke(command, { tabId: "mesh:peer/bad?tab", ptyId: "mesh:peer/bad?tab" }))
        .rejects.toThrow("local fallback is forbidden");
      expect(calls.length).toBe(0);
    },
  );

  it("rejects malformed stream targets before local output subscription", () => {
    expect(() => subscribeTabOutput("mesh:peer/bad?tab", { onOutput() {}, onTruncated() {}, onEnded() {} }))
      .toThrow("local output fallback is forbidden");
  });
});

finish();
