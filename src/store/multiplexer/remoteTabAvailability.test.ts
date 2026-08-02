import { describe, expect, it } from "vitest";
import type { Tab } from "../../lib/types";
import { mergeOrdinaryTabProjection } from "./remoteTabProjection";

const tab: Tab = { id: "one", title: "One", pty_id: "one", tab_type: "Terminal", cwd: null };

describe("mesh-state-changed projection", () => {
  it("keeps identity through disconnect and restores without duplication", () => {
    const available = mergeOrdinaryTabProjection({}, [
      { id: "peer", name: "ROG", authenticated: true, reachable: true, tabs: [tab] },
    ]);
    const reconnecting = mergeOrdinaryTabProjection({}, [
      { id: "peer", name: "ROG", authenticated: true, reachable: false, reconnecting: true, tabs: [tab] },
    ]);
    const recovered = mergeOrdinaryTabProjection({}, [
      { id: "peer", name: "ROG", authenticated: true, reachable: true, tabs: [tab] },
    ]);
    expect(Object.keys(available)).toEqual(["mesh:peer/one"]);
    expect(Object.keys(reconnecting)).toEqual(["mesh:peer/one"]);
    expect(reconnecting["mesh:peer/one"].remote_availability).toBe("reconnecting");
    expect(Object.keys(recovered)).toEqual(["mesh:peer/one"]);
  });

  it("does not claim availability when the authenticated peer API is unreachable", () => {
    const unavailable = mergeOrdinaryTabProjection({}, [
      { id: "peer", name: "ROG", authenticated: true, reachable: false, unavailable: true, tabs: [tab] },
    ]);
    expect(unavailable["mesh:peer/one"].remote_availability).toBe("unavailable");
  });
});
