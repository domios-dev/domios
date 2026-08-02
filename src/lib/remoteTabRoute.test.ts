import { describe, expect, finish, it } from "../test/standaloneTest";
import type { Tab } from "./types";
import { remoteTabActionRequest, remoteTabStreamPath, requireRemoteTabRoute } from "./remoteTabRoute";

const base: Tab = { id: "same", title: "Local", pty_id: "same", tab_type: "Terminal", cwd: null };

describe("remote tab routing", () => {
  it("returns only the authenticated composite owner route", () => {
    expect(requireRemoteTabRoute({
      ...base,
      id: "mesh:peer-a/same",
      remote_peer_id: "peer-a",
      remote_tab_id: "same",
      remote_availability: "available",
    })).toEqual({ peerId: "peer-a", tabId: "same" });
    expect(remoteTabStreamPath({ peerId: "peer-a", tabId: "same" }))
      .toContain("peer=peer-a&path=%2Fapi%2Foutput%2Fstream&tab=mesh%3Apeer-a%2Fsame");
  });

  it("never falls back to a colliding local tab", () => {
    expect(() => requireRemoteTabRoute(base)).toThrow("not an authenticated remote projection");
    expect(() => requireRemoteTabRoute({
      ...base,
      id: "mesh:peer-a/same",
      remote_peer_id: "peer-a",
      remote_tab_id: "same",
      remote_availability: "reconnecting",
    })).toThrow("reconnecting");
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
