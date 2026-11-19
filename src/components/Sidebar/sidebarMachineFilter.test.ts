import { expect, finish, it } from "../../test/standaloneTest";
import type { MultiplexerState, Tab, Team, Workspace } from "../../lib/types";
import { deskWorkspaceMembership, projectBrowsedWorkspaces } from "../../store/multiplexer/pairedWorkspaceProjection";
import { browsedWorkspaceRows } from "./browsedWorkspaceRows";
import { selectSidebarWorkspaceView, sidebarWorkspaceKey, wholeSidebarTeam } from "./sidebarMachineFilter";

it("shows every Team member when any member runs on the chosen machine", () => {
  const members = [{ id: "manager", peer: null }, { id: "worker", peer: "blackstar" }];
  expect(wholeSidebarTeam({ peerId: "blackstar" }, members, member => member.peer)).toBe(members);
  expect(wholeSidebarTeam("local", members, member => member.peer)).toBe(members);
  expect(wholeSidebarTeam({ peerId: "air" }, members, member => member.peer)).toEqual([]);
});

it("mirrors the selected desk with scoped chips, exact Team ownership and surface membership", () => {
  const workspace = (id: string, name: string, sort_order: number, tabs: string[]): Workspace =>
    ({ id, name, sort_order, tabs, active_tab_id: tabs[0] ?? null, cwd: null });
  const tab = (id: string, extra: Partial<Tab> = {}): Tab =>
    ({ id, title: id, tab_type: "Terminal", pty_id: null, cwd: null, ...extra });
  const aC9 = workspace("c9", "Same name", 1, ["local-only"]);
  const aTeam = workspace("a-team", "Atlas", 2, ["atlas", "mesh:b/juniper"]);
  const bC9 = workspace("c9", "Same name", 1, ["attempt", "darwin"]);
  const bBoreal = workspace("b-boreal", "Boreal", 2, ["juniper"]);
  const bSnake = workspace("b-snake", "Snake", 3, ["darwin", "browser", "hooke"]);
  const bEmpty = workspace("empty", "Empty", 4, []);
  const snake: Team = { id: "snake-team", workspace_id: bSnake.id, name: "Snake", created_at: 1, lifecycle: "active" };
  const snapshot: MultiplexerState = {
    active_workspace_id: bSnake.id, focused_tab_id: null,
    workspaces: [bSnake, bEmpty, bC9, bBoreal], teams: [snake],
    tabs: {
      attempt: tab("attempt"),
      juniper: tab("juniper", { role: "Worker", ownership: {
        peer_id: "a", peer_name: "Desk A", controller_tab_id: "atlas", controller_title: "Atlas",
        team: { id: "atlas-team", title: "Atlas" },
      } }),
      darwin: tab("darwin", { role: "Manager", team_id: snake.id }),
      hooke: tab("hooke", { role: "Worker", team_id: snake.id, sort_order: 1 }),
      browser: tab("browser", { team_id: snake.id, parent_tab_id: "darwin", sort_order: 2,
        backing: { kind: "surface", plugin_id: "browser", surface_id: "page", instance_id: "fixture" } }),
    },
  };
  const localTabs = {
    "local-only": tab("local-only"), atlas: tab("atlas", { role: "Manager", team_id: "atlas-team" }),
    "mesh:b/juniper": tab("mesh:b/juniper", { role: "Worker", team_id: "atlas-team",
      remote_peer_id: "b", remote_tab_id: "juniper" }),
  };
  expect(deskWorkspaceMembership([aC9, aTeam], localTabs, [
    { id: "atlas-team", workspace_id: aTeam.id, name: "Atlas", created_at: 1, lifecycle: "active" },
  ]).get(aTeam.id)?.map(member => member.id)).toEqual(["atlas", "mesh:b/juniper"]);

  const peer = projectBrowsedWorkspaces("b", snapshot);
  const candidates = [
    ...[aC9, aTeam].map(workspace => ({ key: sidebarWorkspaceKey(null, workspace.id), peerId: null, workspace })),
    ...peer.map(entry => ({ ...entry, key: sidebarWorkspaceKey("b", entry.workspace.id) })),
  ];
  const aKey = sidebarWorkspaceKey(null, aC9.id);
  const bKey = sidebarWorkspaceKey("b", bC9.id);
  const bView = selectSidebarWorkspaceView(candidates, aKey, { peerId: "b" });
  expect(bView.visible.map(entry => entry.workspace.id)).toEqual(["c9", "b-boreal", "b-snake", "empty"]);
  expect(bView.selected?.workspace.id).toBe(bSnake.id);
  expect(selectSidebarWorkspaceView(candidates, bKey, { peerId: "b" }).selected?.workspace.id).toBe(bC9.id);
  expect(selectSidebarWorkspaceView(candidates, bKey, "all").visible.filter(entry => entry.workspace.name === "Same name")
    .map(entry => entry.key)).toEqual([aKey, bKey]);
  expect(peer[0].tabs.map(({ tab }) => tab.id)).toEqual(["attempt"]);
  expect(peer[0].tabIds?.length).toBe(1);
  const boreal = browsedWorkspaceRows(peer[1].tabs);
  expect(boreal.membersByTeam.get("mesh:a/atlas-team")?.map(({ tab }) => tab.id)).toEqual(["mesh:b/juniper"]);
  expect(boreal.drag.looseTabs).toEqual([]);
  const mirroredSnake = browsedWorkspaceRows(peer[2].tabs);
  expect(mirroredSnake.drag.units).toEqual([{ key: snake.id, kind: "team", tabIds: ["mesh:b/darwin", "mesh:b/browser", "mesh:b/hooke"] }]);
  expect(mirroredSnake.attachmentsByOwner.get("mesh:b/darwin")?.map(tab => tab.id)).toEqual(["mesh:b/browser"]);
});

finish();
