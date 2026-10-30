import { describe, expect, it } from "vitest";
import { myMachinesView, type MyMachinesView } from "./accountMachines";
import { accountCanvas } from "./accountCanvas";
import { machine, macRow, NOW, pair, selfRow, snapshot, targetOf } from "./accountMachines.testdata";
import type { AccountMeshSnapshot } from "./accountMeshClient";
import type { PeerStatus } from "./useMeshData";

function account(state: AccountMeshSnapshot) {
  const view = myMachinesView({ account: "signed_in", snapshot: state, now: NOW });
  if (view.kind !== "account") throw new Error("expected account view");
  return view;
}
const canvas = (view: MyMachinesView, peers: PeerStatus[] = []) => accountCanvas({ view, peers, loadFailed: false, listFailed: false, failures: new Map() });

describe("account installation projection", () => {
  it("renders exactly one row per installation, retaining equal display names", () => {
    const first = machine("one", "BLACKSTAR");
    const second = machine("two", "BLACKSTAR");
    const view = account(snapshot({ registry: { observed_at: NOW, rows: [selfRow, first, first, second] } }));
    expect(view.machines.map(row => row.key)).toEqual(["one", "two"]);
    expect(view.saved).toEqual([]);
  });
  it("never joins a connection by display name or invents a row for an unknown connection", () => {
    const state = snapshot({ pairs: [pair("old-machine", "MacBook", { installation_id: "other-installation" })] });
    const view = account(state);
    expect(view.machines.find(row => row.key === macRow.installation_id)?.pairInstanceId).toBeNull();
    expect(view.saved).toEqual([]);
    expect(view.machines).toHaveLength(2);
  });
  it("joins the registry and live transport by installation identity despite differing names", () => {
    const view = account(snapshot({ pairs: [pair("transport", "old name", { installation_id: macRow.installation_id })] }));
    const row = view.machines.find(row => row.key === macRow.installation_id)!;
    expect(row).toMatchObject({ name: "MacBook", pairInstanceId: "transport", status: "linked" });
    const peer: PeerStatus = { instance_id: "transport", name: "old name", ip: "100.64.0.9", api_port: 7685, online: true, access_level: "full", last_seen: null, relay_protocol_version: 2 };
    const projected = canvas(view, [peer]);
    expect(projected.peerFooters.get("transport")?.row.key).toBe(macRow.installation_id);
    expect(projected.nodes.map(node => node.key)).not.toContain(macRow.installation_id);
    expect(projected.nodes.length + projected.peerFooters.size).toBe(2);
  });
  it("shows the six owner installations once, without inferring a shared identity from names", () => {
    const rows = ["ubunta", "BLACKSTAR", "rollacodebot", "green", "ROG", "air"].map((name, index) => machine(`installation-${index}`, name));
    const state = snapshot({ local_installation_id: rows[0].installation_id, self: { ...snapshot().self!, installation_id: rows[0].installation_id }, registry: { observed_at: NOW, rows },
      pairs: rows.slice(1).map((row, index) => pair(`transport-${index}`, row.display_name!, { installation_id: row.installation_id })) });
    const view = account(state);
    expect(view.machines).toHaveLength(5);
    expect(view.machines.every(row => row.status === "linked")).toBe(true);
    expect(view.self?.installationId).toBe(rows[0].installation_id);
  });
  it("names the sign-in needed on green even when its last heartbeat was online", () => {
    const green = machine("green-installation", "green", { presence: "signed_out" });
    const row = account(snapshot({ registry: { observed_at: NOW, rows: [selfRow, green] }, pairs: [pair("green-peer", "green", { installation_id: green.installation_id })] })).machines[0];
    expect(row).toMatchObject({ status: "signed_out", detail: "Sign in on green", online: false, canSignOut: false });
    expect(row.controls.revoke).toEqual(targetOf(green));
  });
  it("keeps stale active installations visible and removable", () => {
    const stale = machine("stale", "Offline machine", { endpoint: { tailnet_ip: "100.64.0.8", api_port: 7685, expires_at: NOW - 1 } });
    const row = account(snapshot({ registry: { observed_at: NOW, rows: [selfRow, stale] } })).machines[0];
    expect(row.status).toBe("endpoint_stale");
    expect(row.controls.revoke).toEqual(targetOf(stale));
    expect(canvas(account(snapshot({ registry: { observed_at: NOW, rows: [selfRow, stale] } }))).nodes).toHaveLength(1);
  });
  it("shows automatic connection progress without a per-edge action", () => {
    const view = account(snapshot());
    expect(view.machines[0].status).toBe("linking");
    expect(canvas(view).nodes.every(node => node.action === null)).toBe(true);
  });
  it("blocks signed-out local access without showing saved connections", () => {
    const view = myMachinesView({ account: "signed_out", snapshot: snapshot({ traffic: "blocked_signed_out", pairs: [pair("peer", "Old machine")] }), now: NOW });
    expect(view).toMatchObject({ kind: "sign_in", blocked: true, saved: [] });
    expect(canvas(view).nodes).toEqual([]);
  });
  it("preserves rejoin and enrollment refusal causes", () => {
    expect(account(snapshot({ enrollment: "refused", last_refusal: { reason: "generation_mismatch" } })).reauthRequired).toBe(true);
    const view = account(snapshot({ traffic: "plan_limit_locked", enrollment: "refused", last_refusal: { reason: "mesh.machines", limit: 2, active_count: 2 } }));
    expect(view.machineLimit).toEqual({ limit: 2, activeCount: 2 });
    expect(canvas(view).nodes.every(node => node.action?.kind === "plans")).toBe(true);
  });
});

 it("requires registry evidence for removed copy while retaining sign-in recovery", () => {
   const refused = snapshot({ self: null, registry: null, enrollment: "refused", last_refusal: { reason: "revoked" } });
   expect(account(refused).removed).toBe(false);
   expect(account(refused).reauthRequired).toBe(true);
   expect(account(snapshot({ ...refused, registry: { observed_at: NOW, rows: [{ ...selfRow, active: false }] } })).removed).toBe(true);
 });
