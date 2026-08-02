import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import type { Tab } from "../../lib/types";
import { mergeOrdinaryTabProjection } from "../../store/multiplexer/remoteTabProjection";

const tab = (id: string): Tab => ({ id, title: id, pty_id: id, tab_type: "Terminal", cwd: null });

describe("sidebar ordinary remote rows", () => {
  it("uses the same ordered projection and has no RemoteWorkerRow copy", () => {
    const tabs = mergeOrdinaryTabProjection({ local: tab("local") }, [
      { id: "peer", name: "ROG", authenticated: true, reachable: true, tabs: [tab("remote")] },
    ]);
    expect(Object.keys(tabs)).toEqual(["local", "mesh:peer/remote"]);
    const source = readFileSync(new URL("./SidebarWorkspaceTabs.tsx", import.meta.url), "utf8");
    expect(source).not.toContain('import { RemoteWorkerRow }');
    expect(source).not.toContain("<RemoteWorkerRow");
    expect(source).toContain("<SidebarSingleTabRow");
  });
});
