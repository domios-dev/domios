import { describe, expect, it } from "vitest";
import { hasCompositeTabArgument, parseCompositeTabId, remoteTransportCall } from "./remoteTabTransport";

describe("remote tab transport adapter", () => {
  it("routes every local-component action to the composite owner", () => {
    for (const command of [
      "mux_close_tab", "mux_rename_tab", "mux_set_focused_tab", "get_tab_text",
      "get_tab_output", "write_pty_string", "resize_pty", "read_pty", "chat_start",
      "chat_stop", "chat_backfill", "chat_send_text", "chat_send_status",
    ]) {
      const call = remoteTransportCall(command, { tabId: "mesh:peer/shared", ptyId: "mesh:peer/shared" });
      expect(call?.peerId).toBe("peer");
      expect(call?.ordinaryTabId).toBe("mesh:peer/shared");
    }
  });

  it("never treats a colliding local id or malformed owner as remote", () => {
    expect(remoteTransportCall("get_tab_output", { tabId: "shared" })).toBeNull();
    expect(parseCompositeTabId("mesh:peer/shared/extra")).toBeNull();
    expect(parseCompositeTabId("mesh:/shared")).toBeNull();
    expect(hasCompositeTabArgument({ tabId: "mesh:peer/shared" })).toBe(true);
    expect(remoteTransportCall("spawn_pty", { tabId: "mesh:peer/shared" })).toBeNull();
  });
});
