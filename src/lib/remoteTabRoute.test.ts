import { describe, expect, finish, it } from "../test/standaloneTest";
import type { Tab } from "./types";
import { remoteTabActionRequest, remoteTabStreamPath, requireRemoteTabRoute, tabOutputRoute } from "./remoteTabRoute";

const base: Tab = { id: "same", title: "Local", pty_id: "same", tab_type: "Terminal", cwd: null };

describe("remote tab routing", () => {
  it("returns only the authenticated composite owner route", () => {
    expect(requireRemoteTabRoute({
      ...base,
      id: "mesh:peer-a/same",
      remote_peer_id: "peer-a",
      remote_tab_id: "same",
      remote_channels: { control: "available", output: "available", chat: "available" },
    })).toEqual({ peerId: "peer-a", tabId: "same" });
    expect(remoteTabStreamPath({ peerId: "peer-a", tabId: "same" }))
      .toContain("peer=peer-a&path=%2Fapi%2Foutput%2Fstream&tab=mesh%3Apeer-a%2Fsame");
  });

  it("routes every relay terminal through a composite per-tab stream with the full peer UUID", () => {
    const peerId = "928b06dd-1234-4567-890a-123456789abc";
    for (const tabId of ["0885347b", "183546de", "68f196b8", "e5de9ad9"]) {
      const route = tabOutputRoute(tabId, peerId);
      expect(route).toEqual({ peerId, tabId });
      if (!route) throw new Error("Missing relay output route");
      const url = new URL(remoteTabStreamPath(route), "http://gateway.test");
      expect(url.pathname).toBe("/api/mesh/relay-stream");
      expect(url.searchParams.get("peer")).toBe(peerId);
      expect(url.searchParams.get("path")).toBe("/api/output/stream");
      expect(url.searchParams.get("tab")).toBe(`mesh:${peerId}/${tabId}`);
      expect(url.searchParams.has("since")).toBe(false);
      expect(new URL(remoteTabStreamPath(route, "opaque-cursor"), url).searchParams.get("since"))
        .toBe("opaque-cursor");
    }
    expect(tabOutputRoute("mesh:other-peer/same", peerId)).toEqual({ peerId: "other-peer", tabId: "same" });
    expect(tabOutputRoute("same", null)).toBe(null);
    expect(() => tabOutputRoute("mesh:broken", peerId)).toThrow("Invalid remote tab id");
  });

  it("never falls back to a colliding local tab", () => {
    expect(() => requireRemoteTabRoute(base)).toThrow("not an authenticated remote projection");
    expect(requireRemoteTabRoute({
      ...base,
      id: "mesh:peer-a/same",
      remote_peer_id: "peer-a",
      remote_tab_id: "same",
      remote_channels: { control: "reconnecting", output: "reconnecting", chat: "reconnecting" },
    })).toEqual({ peerId: "peer-a", tabId: "same" });
  });

  it("carries the owning peer and remote id for every ordinary action", () => {
    const route = { peerId: "peer-a", tabId: "same" };
    for (const [method, suffix] of [
      ["GET", "/inspect"],
      ["POST", "/focus"],
      ["GET", "/output"],
      ["POST", "/send"],
      ["GET", "/chat"],
    ]) {
      expect(remoteTabActionRequest(route, method, suffix)).toMatchObject({
        ordinaryTabId: "mesh:peer-a/same",
        peerId: "peer-a",
        method,
        suffix,
      });
    }
  });
});

finish();
