import { describe, expect, finish, it } from "../../test/standaloneTest";
import type { Tab } from "../../lib/types";
import { remoteTabsFromOrdinaryProjection } from "../../store/multiplexer/remoteTabProjection";
import { matchesSidebarMachine } from "./sidebarMachineFilter";
import { observedGeneralViewers } from "../../store/multiplexer/pairedWorkspaceProjection";
import { projectAttachedRemoteTab } from "../../lib/remoteTabPortal";
import { sharesObservedMachine } from "./remoteOwnership";

const tab = (id: string): Tab => ({ id, title: id, pty_id: id, tab_type: "Terminal", cwd: null });

describe("sidebar ordinary remote rows", () => {
  it("projects ordinary remote rows in order", () => {
    const remote = { ...tab("mesh:peer/remote"), remote_peer_id: "peer", remote_tab_id: "remote" };
    const tabs = remoteTabsFromOrdinaryProjection([tab("local"), remote]);
    expect(Object.keys(tabs)).toEqual(["mesh:peer/remote"]);
  });

  it("keeps a General viewer in exactly one workspace when attached from the picker", () => {
    const remote = tab("main");
    const view = { viewerId: "mesh:peer/main", tab: remote };
    const peers = [{ peerId: "peer", name: "Desk", availability: "online" as const, sessions: [view, view] }];
    const peer = { peer_id: "peer", peer_name: "Desk" };
    expect(observedGeneralViewers(peers, {}, "general").length).toBe(1);
    const inGeneral = projectAttachedRemoteTab(remote, peer, "general");
    expect(observedGeneralViewers(peers, { [view.viewerId]: inGeneral }, "general")).toMatchObject([{ attached: inGeneral }]);
    const inWorkspace = projectAttachedRemoteTab(remote, peer, "project");
    expect(observedGeneralViewers(peers, { [view.viewerId]: inWorkspace }, "general").length).toBe(0);
  });

  it("filters by physical machine identity and keeps all rows by default", () => {
    const peerIds = [undefined, "peer-a", "peer-b"];
    expect(peerIds.filter(id => matchesSidebarMachine("all", id))).toEqual(peerIds);
    expect(peerIds.filter(id => matchesSidebarMachine("local", id))).toEqual([undefined]);
    expect(peerIds.filter(id => matchesSidebarMachine({ peerId: "peer-b" }, id))).toEqual(["peer-b"]);
  });

  it("hides a Worker machine label only when both rows observe the same host id", () => {
    expect(sharesObservedMachine("peer-a", "peer-a")).toBe(true);
    expect(sharesObservedMachine("peer-a", "peer-b")).toBe(false);
    expect(sharesObservedMachine("peer-a", null)).toBe(false);
    expect(sharesObservedMachine(null, "peer-a")).toBe(false);
  });
});

finish();
