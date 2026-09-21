import { describe, expect, finish, it } from "../test/standaloneTest";
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
    expect(start?.body).toEqual({ subscriptionId: "sub-1" });
    expect(stop?.body).toEqual({ subscriptionId: "sub-1" });
    const claim = remoteTransportCall("claim_resize_owner", {
      tabId: "mesh:peer/shared", ptyId: "mesh:peer/pty-1", clientId: "client-1", cols: 91, rows: 33, force: true,
    });
    expect(claim?.suffix).toBe("/pty/claim-resize");
    expect(claim?.body).toMatchObject({ client_id: "client-1", cols: 91, rows: 33, force: true });
    expect(claim?.select?.({
      owner: { tab_id: "shared", pty_id: "pty-1", client_id: "client-1" },
      current_size: { cols: 80, rows: 24 },
    })).toMatchObject({
      owner: { tab_id: "mesh:peer/shared", pty_id: "mesh:peer/pty-1", client_id: "client-1" },
      current_size: { cols: 80, rows: 24 },
    });
    const resize = remoteTransportCall("resize_pty", {
      tabId: "mesh:peer/shared", ptyId: "mesh:peer/pty-1", clientId: "client-1", cols: 160, rows: 48,
    });
    expect(resize?.suffix).toBe("/pty/resize");
    expect(resize?.body).toEqual({ cols: 160, rows: 48, client_id: "client-1" });
    const send = remoteTransportCall("chat_send_text", {
      tabId: "mesh:peer/shared",
      text: "hello",
      clientToken: "optimistic-1",
    });
    expect(send?.body).toEqual({ text: "hello", client_token: "optimistic-1" });
  });

  it("composite to raw and raw to composite identity mapping both directions", () => {
    const peerId = "home-laptop";
    const rawTabId = "tab-42";
    const composite = `mesh:${peerId}/${rawTabId}`;
    const commands = [
      "mux_close_tab", "mux_rename_tab", "mux_set_focused_tab", "get_tab_text",
      "get_tab_output", "get_tab_history_raw", "write_pty_string", "interrupt_agent",
      "resize_pty", "read_pty", "claim_resize_owner", "chat_start", "chat_stop",
      "chat_backfill", "chat_send_text", "chat_send_status", "shell_command_snapshot",
      "delayed_send_list", "delayed_send_create", "mux_respond_prompt", "restart_agent_tab",
    ];

    // OUTBOUND: every command kind rewrites the composite owner to the raw peer tab id.
    for (const command of commands) {
      const call = remoteTransportCall(command, { tabId: composite, ptyId: composite });
      expect(call).not.toBeNull();
      expect(call?.peerId).toBe(peerId);
      expect(call?.ordinaryTabId).toBe(composite);
      expect(call?.remoteTabId).toBe(rawTabId);
      // send path derives the raw id as ordinary.slice(`mesh:${peer}/`.length); prove they agree.
      expect(call?.ordinaryTabId.slice(`mesh:${peerId}/`.length)).toBe(call?.remoteTabId);
    }

    // INBOUND: parse is the exact inverse, so a raw peer tab id remaps only to its composite owner.
    const identity = parseCompositeTabId(composite);
    expect(identity).not.toBeNull();
    expect(identity?.peerId).toBe(peerId);
    expect(identity?.remoteTabId).toBe(rawTabId);
    expect(identity?.ordinaryTabId).toBe(composite);
    expect(`mesh:${identity?.peerId}/${identity?.remoteTabId}`).toBe(composite);
    expect(parseCompositeTabId(`mesh:${peerId}/${rawTabId}`)?.ordinaryTabId).toBe(composite);

    // MALFORMED: reject in both directions, never silently fall back to a local target.
    for (const bad of ["mesh:peer/shared/extra", "mesh:/shared", "mesh:peer/", "mesh:peer/a b", "mesh:peer/a?b"]) {
      expect(parseCompositeTabId(bad)).toBeNull();
      expect(classifyTabTarget(bad).kind).toBe("invalid-remote");
      expect(remoteTransportCall("get_tab_output", { tabId: bad })).toBeNull();
    }
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

finish();
