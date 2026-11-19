import assert from "node:assert/strict";
import { test } from "node:test";
import { registryMachineNames } from "./registryMachineNames";

const row = (installation_id: string, admitted_at: number, display_name: string, active = true) =>
  ({ installation_id, admitted_at, display_name, active });
const registry = [
  row("later", 20, "BLACKSTAR [dev]"),
  row("tied-b", 10, "BLACKSTAR [air]"),
  row("tied-a", 10, "BLACKSTAR"),
  row("removed", 1, "BLACKSTAR", false),
];

test("duplicate labels are stable across local ordering and use enrollment order then installation id", () => {
  const expected = new Map([["tied-a", "BLACKSTAR #1"], ["tied-b", "BLACKSTAR #2"], ["later", "BLACKSTAR #3"]]);
  for (const rows of [registry, [...registry].reverse(), [registry[1], registry[3], registry[0], registry[2]]]) {
    const { labels, duplicates } = registryMachineNames(rows);
    assert.deepEqual(labels, expected);
    assert.deepEqual(duplicates, [{ name: "BLACKSTAR", installations: ["tied-a", "tied-b", "later"] }]);
  }
});

test("the next registry revision removes suffixes once names are distinct", () => {
  const { labels, duplicates } = registryMachineNames([
    registry[0],
    { ...registry[1], display_name: "AIR" },
    { ...registry[2], display_name: "DESK" },
    registry[3],
  ]);
  assert.deepEqual(labels, new Map([["later", "BLACKSTAR"], ["tied-b", "AIR"], ["tied-a", "DESK"]]));
  assert.deepEqual(duplicates, []);
});
