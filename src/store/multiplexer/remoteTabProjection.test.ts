import { describe, expect, it } from "vitest";
import type { Tab } from "../../lib/types";
import { mergeOrdinaryTabProjection } from "./remoteTabProjection";

const tab = (id: string, title: string): Tab => ({
  id, title, pty_id: id, tab_type: "Terminal", cwd: null,
});

describe("ordinary remote tab projection", () => {
  it("merges only reachable authenticated peers with peer-scoped identity", () => {
    const projection = mergeOrdinaryTabProjection(
      { "shared-tab": tab("shared-tab", "Local") },
      [
        { id: "a", name: "ROG", authenticated: true, reachable: true, tabs: [tab("shared-tab", "A")] },
        { id: "b", name: "Mac", authenticated: true, reachable: true, tabs: [tab("shared-tab", "B")] },
        { id: "unpaired", name: "No", authenticated: false, reachable: true, tabs: [tab("ghost", "Ghost")] },
        { id: "offline", name: "No", authenticated: true, reachable: false, tabs: [tab("ghost", "Ghost")] },
      ],
    );

    expect(Object.keys(projection)).toEqual([
      "shared-tab", "mesh:a/shared-tab", "mesh:b/shared-tab",
    ]);
    expect(projection["mesh:a/shared-tab"].remote_tab_id).toBe("shared-tab");
  });

  it("preserves a reconnecting row without claiming availability", () => {
    const projection = mergeOrdinaryTabProjection({}, [
      { id: "a", name: "ROG", authenticated: true, reachable: false, reconnecting: true, tabs: [tab("one", "One")] },
    ]);
    expect(projection["mesh:a/one"].remote_availability).toBe("reconnecting");
  });
});
