import { describe, expect, it } from "vitest";
import type { Tab } from "../../lib/types";
import { remoteTabsFromOrdinaryProjection } from "./remoteTabProjection";

const projected = (availability: Tab["remote_availability"]): Tab => ({
  id: "mesh:peer/one", title: "One", pty_id: "mesh:peer/one", tab_type: "Terminal", cwd: null,
  remote_peer_id: "peer", remote_tab_id: "one", remote_availability: availability,
});

describe("mesh-state-changed authoritative projection", () => {
  it("keeps one composite identity through reconnect and recovery", () => {
    for (const availability of ["available", "reconnecting", "unavailable", "available"] as const) {
      const tabs = remoteTabsFromOrdinaryProjection([projected(availability)]);
      expect(Object.keys(tabs)).toEqual(["mesh:peer/one"]);
      expect(tabs["mesh:peer/one"].remote_availability).toBe(availability);
    }
  });
});
