import assert from "node:assert/strict";
import test from "node:test";
import { groupSpecimens } from "./navigationModel";
import type { Specimen } from "./types";

const entries: Specimen[] = [
  { id: "row", title: "Tab row", category: "Sidebar", where: "", states: [] },
  { id: "menu", title: "Dropdown", category: "Menus", where: "", states: [] },
  { id: "extra", title: "Custom", category: "Extra", where: "", states: [] },
];

test("search matches titles regardless of casing and surrounding spaces", () => {
  assert.deepEqual(groupSpecimens(entries, "  DROP  ").flatMap((group) => group.entries.map((entry) => entry.id)), ["menu"]);
});

test("search matches categories and removes empty groups", () => {
  assert.deepEqual(groupSpecimens(entries, "sidebar").map((group) => group.category), ["Sidebar"]);
  assert.deepEqual(groupSpecimens(entries, "missing"), []);
});

test("clearing search restores category order and retains additional categories", () => {
  assert.deepEqual(groupSpecimens(entries, " ").map((group) => group.category), ["Menus", "Sidebar", "Extra"]);
});
