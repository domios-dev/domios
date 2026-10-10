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
  remote_channels: { control: "available", output: "available", chat: "available" },
});

describe("remote tab metadata", () => {
  it("keeps peer identity and controller attribution on separate lines", () => {
    expect(remoteIdentity(remote("Worker"))).toEqual({ role: null, peer: "@ROG" });
    expect(remoteIdentity(remote("Manager", "planner"))).toEqual({ role: "Planner", peer: "@ROG" });
    expect(remoteIdentity(remote("Manager", "orchestrator"))).toEqual({ role: "Orchestrator", peer: "@ROG" });
    expect(managedByLabel(remote("Worker"))).toBe("managed by Hubble");
  });

  it("does not mix stream availability into identity", () => {
    expect(remoteIdentity({ ...remote("Worker"), remote_channels: { control: "unavailable", output: "unavailable", chat: "unavailable" } }))
      .toEqual({ role: null, peer: "@ROG" });
  });

  it("renders a named network badge only for physical remotes", () => {
    for (const remotePeer of ["peer", undefined]) {
      const tab = { ...remote("Worker"), remote_peer_id: remotePeer };
      const html = renderToStaticMarkup(<TabItem tab={tab} index={0} isActive onClick={() => {}} onClose={() => {}} canClose workspaceId="workspace" />);
      const badge = 'role="img" aria-label="on @ROG"';
      expect(html.includes(badge)).toBe(Boolean(remotePeer));
      expect(html).toContain(managedByLabel(tab)!);
    }
  });

  it("removes managed-by after ownership is released", () => {
    const released = { ...remote("Worker"), ownership: undefined };
    expect(managedByLabel(released)).toBeNull();
    const html = renderToStaticMarkup(
      <TabItem
        tab={released}
        index={0}
        isActive
        onClick={() => undefined}
        onClose={() => undefined}
        canClose
        workspaceId="workspace"
        hasAgent
      />,
    );
    expect(html).toContain('role="img" aria-label="on @ROG"');
    expect(html).not.toContain("managed by Hubble");
  });
});

finish();
