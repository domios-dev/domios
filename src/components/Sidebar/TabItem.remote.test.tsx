import { describe, expect, finish, it } from "../../test/standaloneTest";
import { renderToStaticMarkup } from "react-dom/server";
import type { Tab } from "../../lib/types";
import { TabItem } from "./TabItem";
import { managedByLabel } from "./remoteOwnership";
import { resetRemoteChatStatus, updateRemoteChatPeers } from "../../lib/remoteChatStatus";

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
  it("includes the machine and kind only for physical remotes", () => {
    for (const remotePeer of ["peer", undefined]) {
      const tab = { ...remote("Worker"), remote_peer_id: remotePeer };
      const html = renderToStaticMarkup(<TabItem tab={tab} index={0} isActive onClick={() => {}} onClose={() => {}} canClose workspaceId="workspace" />);
      expect(html.includes("on ROG, machine")).toBe(Boolean(remotePeer));
      expect(html).toContain(managedByLabel(tab)!);
    }
  });

  it("includes the remote Team and status in the row name", () => {
    const tab = remote("Worker");
    tab.remote_peer_name = "ROG [dev-peer]";
    tab.agent_type = "codex";
    tab.agent_session_id = "worker-session";
    tab.ownership = { ...tab.ownership!, team: { id: "crew-id", title: "Deployments" } };
    const html = renderToStaticMarkup(<TabItem tab={tab} index={0} isActive={false}
      onClick={() => {}} onClose={() => {}} canClose={false} workspaceId="workspace" status="Working" />);
    const rowLabel = html.match(/aria-label="([^"]*One[^"]*)"/)?.[1];
    for (const word of ["Worker", "Deployments", "ROG", "working"]) expect(rowLabel).toContain(word);
    expect(rowLabel).not.toContain("[dev-peer]");
  });

  it("keeps machine information in the remote row accessible name", () => {
    const html = renderToStaticMarkup(<TabItem tab={remote("Worker")} index={0} isActive={false}
      onClick={() => {}} onClose={() => {}} canClose={false} workspaceId="workspace" />);
    expect(html).toContain("on ROG, machine");
  });

  it("names an offline remote row and suppresses work indicators", () => {
    const html = renderToStaticMarkup(<TabItem
      tab={{ ...remote("Manager"), id: "mesh:peer/main", protected: true, remote_peer_id: undefined }}
      index={0} isActive={false} onClick={() => {}} onClose={() => {}} canClose={false} workspaceId="workspace"
      observedMachine={{ peerId: "peer", name: "ROG", availability: "offline" }} readOnly />);
    expect(html).toContain("ROG, machine, offline");
    expect(html).not.toContain("data-testid=\"tab-working-activity\"");
  });

  it("gives an unusable remote row and its activation the machine and state", () => {
    updateRemoteChatPeers({ paired: [], local_instance_name: "Local" });
    try {
      const html = renderToStaticMarkup(<TabItem tab={{ ...remote("Manager", "orchestrator"), title: "Halley" }}
        index={0} isActive={false} onClick={() => {}} onClose={() => {}} canClose workspaceId="workspace" />);
      const rowLabel = html.match(/aria-label="([^"]*Halley[^"]*)"/)?.[1];
      for (const word of ["Orchestrator", "No Team", "ROG", "not paired"]) expect(rowLabel).toContain(word);
      expect(html).not.toContain('data-testid="tab-working-activity"');
    } finally { resetRemoteChatStatus(); }
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
    expect(html).toContain("on ROG, machine");
    expect(html).not.toContain("managed by Hubble");
  });
});

finish();
