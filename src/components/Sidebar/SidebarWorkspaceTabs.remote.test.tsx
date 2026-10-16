import { describe, expect, finish, it } from "../../test/standaloneTest";
import type { Tab } from "../../lib/types";
import { remoteTabsFromOrdinaryProjection } from "../../store/multiplexer/remoteTabProjection";

const tab = (id: string): Tab => ({ id, title: id, pty_id: id, tab_type: "Terminal", cwd: null });

describe("sidebar ordinary remote rows", () => {
  it("projects ordinary remote rows in order", () => {
    const remote = { ...tab("mesh:peer/remote"), remote_peer_id: "peer", remote_tab_id: "remote" };
    const tabs = remoteTabsFromOrdinaryProjection([tab("local"), remote]);
    expect(Object.keys(tabs)).toEqual(["mesh:peer/remote"]);
  });
});

finish();
