import { expect, finish, it } from "../../test/standaloneTest";
import { selectSidebarWorkspaceView, wholeSidebarTeam } from "./sidebarMachineFilter";

it("shows every Team member when any member runs on the chosen machine", () => {
  const members = [{ id: "manager", peer: null }, { id: "worker", peer: "blackstar" }];
  expect(wholeSidebarTeam({ peerId: "blackstar" }, members, member => member.peer)).toBe(members);
  expect(wholeSidebarTeam("local", members, member => member.peer)).toBe(members);
  expect(wholeSidebarTeam({ peerId: "air" }, members, member => member.peer)).toEqual([]);
});

it("keeps workspace records and chooses the first visible workspace when selection is hidden", () => {
  const here = { id: "here", tabs: [null] };
  const firstRemote = { id: "there", tabs: ["blackstar", null] };
  const anotherRemote = { id: "other", tabs: ["blackstar"] };
  const workspaces = [here, firstRemote, anotherRemote];
  const view = selectSidebarWorkspaceView(workspaces, here.id, { peerId: "blackstar" }, workspace => workspace.tabs);
  expect(view.visible).toEqual([firstRemote, anotherRemote]);
  expect(view.selected).toBe(firstRemote);
  expect(view.selected?.tabs).toBe(firstRemote.tabs);
});

finish();
