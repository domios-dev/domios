import assert from "node:assert/strict";
import { test } from "node:test";
import { myMachinesView } from "../components/MeshPanel/accountMachines";
import { pair, snapshot } from "../components/MeshPanel/accountMachines.testdata";
import { accountMachineNameProjection, registryMachineNames } from "./registryMachineNames";

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

test("a foreign durable binding still uses the verified registry of the current account", () => {
  const verified = [row("a", 10, "BLACKSTAR [A]"), row("b", 20, "BLACKSTAR [B]"), row("air", 30, "AIR")];
  const expected = new Map([["a", "BLACKSTAR #1"], ["b", "BLACKSTAR #2"], ["air", "AIR"]]);
  for (const [local, binding] of [["a", "current"], ["b", "other"], ["air", "current"]] as const) {
    const otherMachines = verified.filter(row => row.installation_id !== local);
    const account = snapshot({
      local_installation_id: local,
      binding,
      self: null,
      registry_names: [...verified].reverse(),
      pairs: otherMachines.map(row => pair(row.installation_id, "Unverified name")),
    });
    const names = accountMachineNameProjection(account);
    const mesh = myMachinesView({ account: "signed_in", snapshot: account, now: 30 });
    assert.equal(mesh.kind, "account");
    assert.equal(mesh.registrySelfName, expected.get(local));
    assert.equal(names.local, expected.get(local));
    assert.deepEqual(names.peers, new Map(otherMachines.map(row => [row.installation_id, expected.get(row.installation_id)])));
  }

  const withoutAdmission = accountMachineNameProjection(snapshot({ binding: "other", registry_names: [] }));
  assert.equal(withoutAdmission.local, undefined);
  assert.equal(withoutAdmission.peers.size, 0);
});
