import { describe, expect, it } from "vitest";
import type { Tab } from "./types";
import { requireRemoteTabRoute } from "./remoteTabRoute";

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
});
