// Run: npx tsx src/components/Sidebar/sidebarDragModel.test.ts

import { describe, expect, finish, it } from "../../test/standaloneTest";
import type { Tab } from "../../lib/types";
import { buildSidebarDragModel, unitsInOrder, withSiblingOrder, workspaceTabOrder, type DragUnit } from "./sidebarDragModel";
import { surfaceAttachmentsByOwner } from "./surfaceAttachmentModel";

const TOP = ["general", "solo-a", "lead", "w1", "w2", "solo-b", "hidden"];
const UNITS: DragUnit[] = [
  { key: "general", kind: "tab", tabIds: ["general"] },
  { key: "solo-a", kind: "tab", tabIds: ["solo-a"] },
  { key: "team-1", kind: "team", tabIds: ["lead", "w1", "w2"] },
  { key: "solo-b", kind: "tab", tabIds: ["solo-b"] },
];

interface TabSpec {
  id: string;
  order: number;
  parent?: string;
  team?: string;
  role?: "Manager" | "Worker";
  profile?: string;
  surface?: boolean;
}

function makeTabs(specs: readonly TabSpec[]): Tab[] {
  return specs.map((spec) => ({
    id: spec.id,
    title: spec.id,
    sort_order: spec.order,
    parent_tab_id: spec.parent ?? null,
    team_id: spec.team ?? null,
    role: spec.role ?? null,
    role_profile: spec.profile ?? null,
    ...(spec.surface ? { backing: { kind: "surface" } } : {}),
  }) as unknown as Tab);
}

function modelOf(tabs: readonly Tab[]) {
  const sorted = [...tabs].sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));
  return { ids: sorted.map((tab) => tab.id), model: buildSidebarDragModel(sorted, surfaceAttachmentsByOwner(sorted)) };
}

function displayed(tabs: readonly Tab[]): string[] {
  return modelOf(tabs).model.units.flatMap((unit) => unit.tabIds);
}

function applyOrder(tabs: readonly Tab[], order: readonly string[]): Tab[] {
  return tabs.map((tab) => ({ ...tab, sort_order: order.indexOf(tab.id) }));
}

// Ordinals as the per-level ops left them: children numbered from 0 under a top-level orchestrator.
const TEAM_WITH_CHILD_MEMBERS = makeTabs([
  { id: "general", order: 0 },
  { id: "joyce", order: 1, team: "t", role: "Manager", profile: "orchestrator" },
  { id: "gauss", order: 0, parent: "joyce", team: "t", role: "Worker" },
  { id: "builder", order: 1, parent: "joyce", team: "t", role: "Worker" },
  { id: "verifier", order: 2, parent: "joyce", team: "t", role: "Worker" },
  { id: "server", order: 3, parent: "joyce", team: "t" },
  { id: "gauss-web", order: 4, parent: "gauss", surface: true },
  { id: "loose", order: 2 },
]);

const LOOSE_TREE = makeTabs([
  { id: "general", order: 0 },
  { id: "parent", order: 1 },
  { id: "child-a", order: 0, parent: "parent" },
  { id: "child-b", order: 1, parent: "parent" },
  { id: "parent-web", order: 5, parent: "parent", surface: true },
  { id: "solo", order: 2 },
]);

describe("sidebar drag model", () => {
  it("moves a whole team as one block and keeps every workspace tab", () => {
    const order = workspaceTabOrder(TOP, unitsInOrder(UNITS, ["general", "team-1", "solo-a", "solo-b"]));
    expect(order).toEqual(["general", "lead", "w1", "w2", "solo-a", "solo-b", "hidden"]);
  });

  it("reorders a team section without moving the team or other tabs", () => {
    const order = workspaceTabOrder(TOP, withSiblingOrder(UNITS, ["w2", "w1"], (id) => [id]));
    expect(order).toEqual(["general", "solo-a", "lead", "w2", "w1", "solo-b", "hidden"]);
  });

  it("moves a single tab among units, never into a team block", () => {
    const order = workspaceTabOrder(TOP, unitsInOrder(UNITS, ["general", "solo-b", "solo-a", "team-1"]));
    expect(order).toEqual(["general", "solo-b", "solo-a", "lead", "w1", "w2", "hidden"]);
  });

  it("ignores ids that are not tabs of the workspace", () => {
    const order = workspaceTabOrder(["a", "b"], [
      { key: "x", kind: "tab", tabIds: ["foreign", "b"] },
      { key: "a", kind: "tab", tabIds: ["a"] },
    ]);
    expect(order).toEqual(["b", "a"]);
  });

  it("expands a team block over child members and their attachments", () => {
    const { model } = modelOf(TEAM_WITH_CHILD_MEMBERS);
    expect(model.units.map((unit) => unit.key)).toEqual(["general", "t", "loose"]);
    expect(model.units[1].tabIds).toEqual(["joyce", "gauss", "gauss-web", "builder", "verifier", "server"]);
  });

  it("keeps a worker section reorder when members are children of the orchestrator", () => {
    const { ids, model } = modelOf(TEAM_WITH_CHILD_MEMBERS);
    const order = workspaceTabOrder(ids, withSiblingOrder(model.units, ["verifier", "gauss", "builder"], model.blockOf));
    expect(order).toEqual(["general", "joyce", "verifier", "gauss", "gauss-web", "builder", "server", "loose"]);
    expect(displayed(applyOrder(TEAM_WITH_CHILD_MEMBERS, order))).toEqual(order);
  });

  it("moves a team block below a loose tab even when a child member had a smaller ordinal", () => {
    const { ids, model } = modelOf(TEAM_WITH_CHILD_MEMBERS);
    const order = workspaceTabOrder(ids, unitsInOrder(model.units, ["general", "loose", "t"]));
    expect(order).toEqual(["general", "loose", "joyce", "gauss", "gauss-web", "builder", "verifier", "server"]);
    expect(displayed(applyOrder(TEAM_WITH_CHILD_MEMBERS, order))).toEqual(order);
  });

  it("moves a loose tab with its children and attachments as one unit", () => {
    const { ids, model } = modelOf(LOOSE_TREE);
    expect(model.units.map((unit) => unit.key)).toEqual(["general", "parent", "solo"]);
    const order = workspaceTabOrder(ids, unitsInOrder(model.units, ["general", "solo", "parent"]));
    expect(order).toEqual(["general", "solo", "parent", "parent-web", "child-a", "child-b"]);
    expect(displayed(applyOrder(LOOSE_TREE, order))).toEqual(order);
  });

  it("reorders loose children inside their parent's block through the display order", () => {
    const { ids, model } = modelOf(LOOSE_TREE);
    const order = workspaceTabOrder(ids, withSiblingOrder(model.units, ["child-b", "child-a"], model.blockOf));
    expect(order).toEqual(["general", "parent", "parent-web", "child-b", "child-a", "solo"]);
    expect(displayed(applyOrder(LOOSE_TREE, order))).toEqual(order);
  });

  it("appends tabs no unit renders after the displayed ones", () => {
    const tabs = [...LOOSE_TREE, ...makeTabs([{ id: "hidden", order: 0 }])];
    const visible = tabs.filter((tab) => tab.id !== "hidden");
    const { model } = modelOf(visible);
    const ids = modelOf(tabs).ids;
    const order = workspaceTabOrder(ids, unitsInOrder(model.units, ["solo", "general", "parent"]));
    expect(order).toEqual(["solo", "general", "parent", "parent-web", "child-a", "child-b", "hidden"]);
  });
});

finish();
