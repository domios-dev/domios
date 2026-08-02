import { afterEach, describe, expect, finish, it, vi } from "../test/standaloneTest";
import { __setInvokeForTest, invoke } from "./bridge";
import { subscribeTabOutput } from "./outputStream";

afterEach(() => __setInvokeForTest(null));

describe("bridge remote-shaped target guard", () => {
  it.each(["mux_set_focused_tab", "mux_close_tab", "mux_rename_tab", "get_tab_text", "write_pty_string"])(
    "rejects malformed mesh target before %s reaches the local bridge",
    async (command) => {
      const local = vi.fn();
      __setInvokeForTest(local);
      await expect(invoke(command, { tabId: "mesh:peer/bad?tab", ptyId: "mesh:peer/bad?tab" }))
        .rejects.toThrow("local fallback is forbidden");
      expect(local).not.toHaveBeenCalled();
    },
  );

  it("rejects malformed stream targets before local output subscription", () => {
    expect(() => subscribeTabOutput("mesh:peer/bad?tab", { onOutput() {}, onTruncated() {}, onEnded() {} }))
      .toThrow("local output fallback is forbidden");
  });
});

finish();
