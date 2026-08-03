import { describe, expect, finish, it } from "../../test/standaloneTest";
import { renderToStaticMarkup } from "react-dom/server";
import type { Tab } from "../../lib/types";
import { TabItem } from "./TabItem";
import { managedByLabel, remoteIdentity } from "./remoteOwnership";

const remote = (role?: Tab["role"], roleProfile?: string): Tab => ({
  id: "mesh:peer/one",
  title: "One",
  pty_id: "one",
  tab_type: "Terminal",
  cwd: null,
  role,
  role_profile: roleProfile,
  remote_peer_id: "peer",
  remote_peer_name: "ROG",
  remote_tab_id: "one",
  ownership: { controller_title: "Hubble", peer_id: "peer", peer_name: "ROG" },
  remote_availability: "available",
});

finish();

describe("remote tab metadata", () => {
  it("keeps peer identity and controller attribution on separate lines", () => {
    expect(remoteIdentity(remote("Worker"))).toEqual({ role: null, peer: "@ROG" });
    expect(remoteIdentity(remote("Manager", "planner"))).toEqual({ role: "Planner", peer: "@ROG" });
    expect(remoteIdentity(remote("Orchestrator"))).toEqual({ role: "Orchestrator", peer: "@ROG" });
    expect(managedByLabel(remote("Worker"))).toBe("managed by Hubble");
  });

  it("does not mix stream availability into identity", () => {
    expect(remoteIdentity({ ...remote("Worker"), remote_availability: "unavailable" }))
      .toEqual({ role: null, peer: "@ROG" });
  });

  it("renders peer and managed-by as two distinct visual rows", () => {
    const html = renderToStaticMarkup(
      <TabItem
        tab={remote("Manager", "planner")}
        index={0}
        isActive
        onClick={() => undefined}
        onClose={() => undefined}
        canClose
        workspaceId="workspace"
        hasAgent
      />,
    );
    expect(html).toContain('data-testid="remote-tab-identity"');
    expect(html).toContain("PLANNER");
    expect(html).toContain("@ROG");
    expect(html).toContain('data-testid="remote-managed-by"');
    expect(html).toContain("managed by Hubble");
  });
});
