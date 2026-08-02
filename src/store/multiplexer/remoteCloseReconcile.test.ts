import { describe, expect, it } from "vitest";
import type { Tab } from "../../lib/types";
import { remoteTabsFromOrdinaryProjection } from "./remoteTabProjection";

const remote: Tab = {
  id: "mesh:peer/one", title: "One", pty_id: "mesh:peer/one", tab_type: "Terminal", cwd: null,
  remote_peer_id: "peer", remote_tab_id: "one", remote_availability: "available",
};

describe("failed remote close reconciliation", () => {
  it("restores the optimistically removed row from the authoritative projection", () => {
    const optimistic: Record<string, Tab> = {};
    expect(optimistic[remote.id]).toBeUndefined();
    const restored = remoteTabsFromOrdinaryProjection([remote]);
    expect(restored[remote.id]).toEqual(remote);
  });
});
