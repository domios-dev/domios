// Run: npx tsx src/components/Sidebar/sidebarDragModel.test.ts

import { describe, expect, finish, it } from "../../test/standaloneTest";
import { unitsInOrder, withTeamSection, workspaceTabOrder } from "./sidebarDragModel";

const TOP = ["general", "solo-a", "lead", "w1", "w2", "solo-b", "hidden"];
const UNITS = [
  { key: "general", tabIds: ["general"] },
  { key: "solo-a", tabIds: ["solo-a"] },
  { key: "team-1", tabIds: ["lead", "w1", "w2"] },
  { key: "solo-b", tabIds: ["solo-b"] },
];

describe("sidebar drag model", () => {
  it("moves a whole team as one block and keeps every top-level tab", () => {
    const order = workspaceTabOrder(TOP, unitsInOrder(UNITS, ["general", "team-1", "solo-a", "solo-b"]));
    expect(order).toEqual(["general", "lead", "w1", "w2", "solo-a", "solo-b", "hidden"]);
  });

  it("reorders a team section without moving the team or other tabs", () => {
    const sections = [["lead"], ["w1", "w2"], []];
    const order = workspaceTabOrder(TOP, withTeamSection(UNITS, "team-1", sections, 1, ["w2", "w1"]));
    expect(order).toEqual(["general", "solo-a", "lead", "w2", "w1", "solo-b", "hidden"]);
  });

  it("moves a single tab among units, never into a team block", () => {
    const order = workspaceTabOrder(TOP, unitsInOrder(UNITS, ["general", "solo-b", "solo-a", "team-1"]));
    expect(order).toEqual(["general", "solo-b", "solo-a", "lead", "w1", "w2", "hidden"]);
  });

  it("ignores ids that are not top-level tabs of the workspace", () => {
    const order = workspaceTabOrder(["a", "b"], [{ key: "x", tabIds: ["child", "b"] }, { key: "a", tabIds: ["a"] }]);
    expect(order).toEqual(["b", "a"]);
  });
});

finish();
