import { describe, expect, it } from "vitest";
import { classifyTabTarget, hasCompositeTabArgument, parseCompositeTabId, remoteTransportCall } from "./remoteTabTransport";

describe("remote tab transport adapter", () => {
  it("routes every local-component action to the composite owner", () => {
    for (const command of [
      "mux_close_tab", "mux_rename_tab", "mux_set_focused_tab", "get_tab_text",
      "get_tab_output", "write_pty_string", "resize_pty", "read_pty", "chat_start",
      "chat_stop", "chat_backfill", "chat_send_text", "chat_send_status", "claim_resize_owner",
    ]) {
      const call = remoteTransportCall(command, { tabId: "mesh:peer/shared", ptyId: "mesh:peer/shared" });
      expect(call?.peerId).toBe("peer");
      expect(call?.ordinaryTabId).toBe("mesh:peer/shared");
    }
  });

  it("maps chat lifecycle and real resize ownership to exact remote REST bodies", () => {
    const start = remoteTransportCall("chat_start", { tabId: "mesh:peer/shared", subscriptionId: "sub-1" });
    const stop = remoteTransportCall("chat_stop", { tabId: "mesh:peer/shared", subscriptionId: "sub-1" });
    expect(start?.body).toEqual({ subscription_id: "sub-1" });
    expect(stop?.body).toEqual({ subscription_id: "sub-1" });
    const claim = remoteTransportCall("claim_resize_owner", {
      tabId: "mesh:peer/shared", ptyId: "mesh:peer/shared", clientId: "client-1", cols: 91, rows: 33,
    });
    expect(claim?.suffix).toBe("/pty/claim-resize");
    expect(claim?.body).toMatchObject({ client_id: "client-1", cols: 91, rows: 33 });
  });

  it("never treats a colliding local id or malformed owner as remote", () => {
    expect(remoteTransportCall("get_tab_output", { tabId: "shared" })).toBeNull();
    expect(parseCompositeTabId("mesh:peer/shared/extra")).toBeNull();
    expect(parseCompositeTabId("mesh:/shared")).toBeNull();
    for (const id of ["mesh:peer/a b", "mesh:peer/a?b", "mesh:peer/a&b", "mesh:peer/a#b"]) {
      expect(parseCompositeTabId(id)).toBeNull();
      expect(classifyTabTarget(id).kind).toBe("invalid-remote");
      expect(hasCompositeTabArgument({ tabId: id })).toBe(true);
    }
    expect(hasCompositeTabArgument({ tabId: "mesh:peer/shared" })).toBe(true);
    expect(remoteTransportCall("spawn_pty", { tabId: "mesh:peer/shared" })).toBeNull();
  });
});
