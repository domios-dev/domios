import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { accountCanvas } from "./accountCanvas";
import { myMachinesView } from "./accountMachines";
import { macRow, NOW, selfRow, snapshot } from "./accountMachines.testdata";
import { meshOverviewState } from "./meshOverviewModel";
import type { TailscaleState } from "./tailscaleSetup";

const online: TailscaleState = { kind: "connected", tailnet: null };
const empty = snapshot({ registry: { observed_at: NOW, rows: [selfRow] } });
const accountView = (state: ReturnType<typeof snapshot> | null, signedIn = true, listFailed = false) => accountCanvas({
  view: myMachinesView({ account: signedIn ? "signed_in" : "signed_out", snapshot: state, now: NOW }),
  peers: [],
  loadFailed: false,
  listFailed,
  failures: new Map(),
});

describe("Mesh overview", () => {
  it("shows sign-in without waiting for a machine snapshot or Tailscale", () => {
    assert.deepEqual(meshOverviewState(accountView(null, false), [], { kind: "not_running" }), { kind: "signed_out" });
  });

  it("distinguishes an empty account, local Tailscale being off, and a list failure", () => {
    assert.deepEqual(meshOverviewState(accountView(empty), [], online), { kind: "only_this_machine" });
    assert.deepEqual(meshOverviewState(accountView(empty), [], { kind: "permission_needed" }), { kind: "tailscale_off" });
    assert.deepEqual(meshOverviewState(accountView(empty, true, true), [], online), { kind: "inventory" });
    assert.deepEqual(meshOverviewState(accountView(empty), [], online, true), { kind: "inventory" });
  });

  it("keeps an unreachable signed-in machine visible and preserves the inventory for a signed-out peer", () => {
    const remote = (row: typeof macRow) => snapshot({
      registry: { observed_at: NOW, rows: [selfRow, row] },
      connections: [{ installation_id: row.installation_id, phase: "failed" }],
    });
    const state = meshOverviewState(accountView(remote(macRow)), [], online);
    assert.equal(state.kind, "unreachable");
    assert.equal(state.machine.key, macRow.installation_id);
    assert.deepEqual(meshOverviewState(accountView(remote({ ...macRow, presence: "signed_out" })), [], online), { kind: "inventory" });
  });
});
