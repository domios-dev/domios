import { readFileSync } from "node:fs";
import { describe, expect, finish, it } from "../../test/standaloneTest";
import type { Tab } from "../../lib/types";
import { remoteTabsFromOrdinaryProjection } from "../../store/multiplexer/remoteTabProjection";

const tab = (id: string): Tab => ({ id, title: id, pty_id: id, tab_type: "Terminal", cwd: null });

describe("sidebar ordinary remote rows", () => {
  it("uses the same ordered projection and has no RemoteWorkerRow copy", () => {
    const remote = { ...tab("mesh:peer/remote"), remote_peer_id: "peer", remote_tab_id: "remote" };
    const tabs = remoteTabsFromOrdinaryProjection([tab("local"), remote]);
    expect(Object.keys(tabs)).toEqual(["mesh:peer/remote"]);
    const source = readFileSync(new URL("./SidebarWorkspaceTabs.tsx", import.meta.url), "utf8");
    expect(source).not.toContain('import { RemoteWorkerRow }');
    expect(source).not.toContain("<RemoteWorkerRow");
    expect(source).toContain("<SidebarSingleTabRow");
  });
});

finish();
