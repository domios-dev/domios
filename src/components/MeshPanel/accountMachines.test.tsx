import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vitest";
import MyMachines from "./AddPeerPanel";
import {
  accountPresence,
  failureMessage,
  myMachinesView,
  settledLinks,
  PLAN_LOCKED_COPY,
  SIGN_IN_COPY,
  SIGNED_OUT_COPY,
  type MachineRowView,
  type MyMachinesView,
  type AccountPresence,
} from "./accountMachines";
import { AccountMeshClientProvider, type AccountMeshSnapshot } from "./accountMeshClient";
import { airRow, linkRequest, FakeAccountMeshClient, lenovoRow, machine, macRow, NOW, pair, selfRow, snapshot, targetOf } from "./accountMachines.testdata";

const none = new Set<string>();
const view = (account: AccountPresence, state: AccountMeshSnapshot | null, linking: ReadonlySet<string> = none) =>
  myMachinesView({ account, snapshot: state, linking, now: NOW });

function accountView(result: MyMachinesView) {
  if (result.kind !== "account") throw new Error(`expected account view, got ${result.kind}`);
  return result;
}
function signInView(result: MyMachinesView) {
  if (result.kind !== "sign_in") throw new Error(`expected sign-in view, got ${result.kind}`);
  return result;
}
const byKey = (rows: MachineRowView[], key: string) => {
  const row = rows.find((candidate) => candidate.key === key);
  if (!row) throw new Error(`row ${key} missing`);
  return row;
};
const noControls = (row: MachineRowView) => !row.controls.add && !row.controls.revoke && !row.controls.unpairInstanceId;

describe("My machines projection", () => {
  it("keeps admitted peers normal when only local enrollment hits the plan limit", () => {
    const result = accountView(view("signed_in", snapshot({
      traffic: "plan_limit_locked", enrollment: "refused",
      last_refusal: { reason: "mesh.machines", limit: 2, active_count: 2 },
    })));
    expect(result.machineLimit).toEqual({ limit: 2, activeCount: 2 });
    expect(result.self?.planLimited).toBe(true);
    expect(result.machines.map(row => row.status)).toEqual(["unlinked", "unlinked"]);
    expect(result.machines.map(row => [row.controls.add, row.controls.plans, row.controls.revoke])).toEqual([
      [null, true, targetOf(macRow)], [null, true, targetOf(lenovoRow)],
    ]);
  });
  it("waits for account and registry observations before projecting", () => {
    expect(view("pending", snapshot()).kind).toBe("loading");
    expect(view("signed_in", null).kind).toBe("loading");
    expect(accountPresence("error")).toBe("signed_out");
  });

  it("identifies this installation and lists other account machines by installation id", () => {
    const result = accountView(view("signed_in", snapshot()));
    expect(result.self).toEqual({ name: "Studio PC", installationId: selfRow.installation_id, metadataRevision: 2, displayName: "Studio PC", planLimited: false });
    expect(result.machines.map((row) => row.key)).toEqual([macRow.installation_id, lenovoRow.installation_id]);
    const mac = byKey(result.machines, macRow.installation_id);
    expect(mac.status).toBe("unlinked");
    expect(mac.online).toBeNull();
    expect(mac.controls.add).toEqual({ reason: null });
    expect(mac.controls.revoke).toEqual(targetOf(macRow));
    expect(mac.controls.unpairInstanceId).toBeNull();
  });

  it("joins a local pair to its registry row as linked and drops Add", () => {
    const linked = snapshot({ pairs: [pair("mesh-mac", "mac.local", { installation_id: macRow.installation_id, online: false })] });
    const mac = byKey(accountView(view("signed_in", linked)).machines, macRow.installation_id);
    expect(mac.status).toBe("linked");
    expect(mac.online).toBe(false);
    expect(mac.controls.add).toBeNull();
    expect(mac.controls.unpairInstanceId).toBe("mesh-mac");
  });

  it("uses explicit presence before expired or stale endpoints, and restores actions on present", () => {
    const remote = machine("inst-away-f", "Away", { presence: "signed_out", membership_expires_at: NOW - 1 });
    const state = snapshot({ registry: { observed_at: NOW, rows: [selfRow, remote] }, pairs: [pair("mesh-away", "Away", { installation_id: remote.installation_id, online: true })] });
    const row = accountView(view("signed_in", state)).machines[0];
    expect(row.status).toBe("signed_out");
    expect(row.detail).toBe("Away is signed out");
    expect(noControls(row)).toBe(true);
    expect(row.online).toBe(false);
    remote.presence = "present";
    expect(accountView(view("signed_in", state)).machines[0].status).toBe("expired");
    remote.membership_expires_at = NOW + 60;
    const present = accountView(view("signed_in", state)).machines[0];
    expect(present.status).toBe("linked");
    expect(present.controls.unpairInstanceId).toBe("mesh-away");
  });

  it("never treats registry metadata as reachability", () => {
    const rows = [
      selfRow,
      airRow,
      machine("inst-stale-e", "Stale", { endpoint: { tailnet_ip: "100.64.0.5", api_port: 7685, expires_at: NOW } }),
      machine("inst-expired-f", "Expired", { membership_expires_at: NOW - 1 }),
      machine("inst-revoked-a", "Revoked", { eligibility: "revoked", active: false }),
      machine("inst-limit-b", "Limit", { eligibility: "mesh-allowance" }),
      machine("inst-unverified-c", "Unverified", { eligibility: "verification-unavailable" }),
    ];
    const result = accountView(view("signed_in", snapshot({ registry: { observed_at: NOW, rows } })));
    const statuses = Object.fromEntries(result.machines.map((row) => [row.name, row.status]));
    expect(statuses).toEqual({
      Air: "endpoint_missing",
      Stale: "endpoint_stale",
      Expired: "expired",
      Revoked: "revoked",
      Limit: "over_limit",
      Unverified: "unverified",
    });
    for (const row of result.machines) {
      expect(row.controls.add === null || typeof row.controls.add.reason === "string").toBe(true);
      expect(row.online).toBeNull();
    }
    expect(byKey(result.machines, "inst-revoked-a").controls.revoke).toBeNull();
  });

  it("keeps Add pending until the joined pair is observed", () => {
    const linking = new Set([macRow.installation_id]);
    const pending = byKey(accountView(view("signed_in", snapshot(), linking)).machines, macRow.installation_id);
    expect(pending.status).toBe("linking");
    expect(pending.controls.add?.reason).toBeTruthy();
    expect(pending.controls.revoke).toBeNull();
    expect(settledLinks(linking, snapshot())).toEqual([]);
    const observed = snapshot({ pairs: [pair("mesh-mac", "mac", { installation_id: macRow.installation_id })] });
    expect(settledLinks(linking, observed)).toEqual([macRow.installation_id]);
  });

  it("disables Add until this machine is enrolled", () => {
    const mac = byKey(accountView(view("signed_in", snapshot({ self: null }))).machines, macRow.installation_id);
    expect(mac.controls.add?.reason).toBeTruthy();
  });

  it("asks a never-signed-in installation to sign in and reports legacy pairs truthfully", () => {
    const legacy = pair("mesh-old", "Old laptop", { admission: "legacy", online: true });
    const result = signInView(view("signed_out", snapshot({ binding: "none", self: null, registry: null, pairs: [legacy] })));
    expect(result.headline).toBe(SIGN_IN_COPY);
    expect(result.blocked).toBe(false);
    expect(result.saved[0].status).toBe("legacy");
    expect(result.saved[0].detail).toContain("online");
    expect(noControls(result.saved[0])).toBe(true);
  });

  it("explicit sign-out keeps every saved pairing as blocked without controls", () => {
    const blocked = snapshot({
      traffic: "blocked_signed_out",
      cleanup: "pending",
      pairs: [pair("mesh-mac", "MacBook", { installation_id: macRow.installation_id }), pair("mesh-old", "Old", { admission: "legacy" })],
    });
    const result = signInView(view("signed_out", blocked));
    expect(SIGNED_OUT_COPY).toBe("Signed out — sign in to use the mesh");
    expect(result.headline).toBe(SIGNED_OUT_COPY);
    expect(result.saved.map((row) => [row.status, row.detail])).toEqual([["blocked", SIGNED_OUT_COPY], ["blocked", SIGNED_OUT_COPY]]);
    expect(result.saved.every(noControls)).toBe(true);
    expect(result.notes.some((note) => note.includes("slot"))).toBe(true);
    expect(result.notes.some((note) => note.includes("cleanup is pending"))).toBe(true);
  });

  it("a plan-locked machine keeps saved pair identities without marking them over limit", () => {
    const locked = snapshot({ traffic: "plan_limit_locked", pairs: [pair("mesh-old", "Old", { admission: "legacy" })] });
    const result = accountView(view("signed_in", locked));
    expect(result.saved.map((row) => [row.key, row.status, row.controls.unpairInstanceId])).toEqual([["pair:mesh-old", "legacy", "mesh-old"]]);
    expect(result.self?.planLimited).toBe(true);
    expect(failureMessage({ reason: "plan_limit_locked" })).toBe(PLAN_LOCKED_COPY);
  });

  it("same-account sign-in restores the same row identity with usable controls", () => {
    const saved = pair("mesh-mac", "MacBook", { installation_id: macRow.installation_id });
    const signedOut = signInView(view("signed_out", snapshot({ traffic: "blocked_signed_out", pairs: [saved] })));
    const signedIn = accountView(view("signed_in", snapshot({ pairs: [saved] })));
    const restored = byKey(signedIn.machines, macRow.installation_id);
    expect(signedOut.saved[0].key).toBe(restored.key);
    expect(restored.status).toBe("linked");
    expect(restored.controls.unpairInstanceId).toBe("mesh-mac");
  });

  it("a different account never inherits controls over saved pairings", () => {
    const saved = pair("mesh-mac", "MacBook", { installation_id: macRow.installation_id });
    const result = accountView(view("signed_in", snapshot({ binding: "other", pairs: [saved] })));
    expect(result.saved.map((row) => row.status)).toEqual(["other_account"]);
    expect(result.saved.every(noControls)).toBe(true);
    for (const row of result.machines) {
      expect(row.controls.revoke).toBeNull();
      expect(row.controls.unpairInstanceId).toBeNull();
      expect(row.controls.add?.reason ?? null).not.toBeNull();
    }
  });

  it("keeps known rows when a registry refresh fails", () => {
    const result = accountView(view("signed_in", snapshot({ registry_failure: { reason: "unavailable" } })));
    expect(result.machines).toHaveLength(2);
    expect(result.notes.some((note) => note.startsWith("Showing the last account list"))).toBe(true);
  });
});

describe("My machines account transitions", () => {
  it("re-reads account Mesh state when the account changes and swaps the sign-in prompt for the list", async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    const saved = pair("mesh-mac", "MacBook", { installation_id: macRow.installation_id });
    const client = new FakeAccountMeshClient(snapshot({ traffic: "blocked_signed_out", pairs: [saved] }));
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    const render = (account: AccountPresence) => root.render(
      <AccountMeshClientProvider value={client}>
        <MyMachines account={account} onSignIn={() => {}} onFailure={() => {}} now={() => NOW} />
      </AccountMeshClientProvider>,
    );
    try {
      await act(async () => render("signed_out"));
      expect(host.querySelector('[role="status"]')?.textContent).toBe(SIGNED_OUT_COPY);
      const blockedRow = host.querySelector('ul[aria-label="Saved pairings"] li[aria-label="MacBook"]');
      expect(blockedRow?.getAttribute("data-status")).toBe("blocked");
      expect(blockedRow?.querySelector("button")).toBeNull();

      client.state = snapshot({ pairs: [saved] });
      await act(async () => render("signed_in"));
      const linkedRow = host.querySelector('ul[aria-label="Your account\'s machines"] li[aria-label="MacBook"]');
      expect(linkedRow?.getAttribute("data-status")).toBe("linked");
      expect(linkedRow?.querySelector('button[aria-label="Disconnect MacBook"]')).not.toBeNull();

      await act(async () => client.emit(snapshot({ binding: "other", pairs: [saved] })));
      const foreignRow = host.querySelector('ul[aria-label="Saved pairings"] li[aria-label="MacBook"]');
      expect(foreignRow?.getAttribute("data-status")).toBe("other_account");
      expect(foreignRow?.querySelector("button")).toBeNull();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});

it("shows a publication refusal and the scheduled retry in mesh status", () => {
  const result = accountView(view("signed_in", snapshot({
    publication: "published",
    publication_failure: { reason: "rate-limited", retry_after_seconds: 7 },
    retry_at: NOW + 7,
  })));
  expect(result.notes.join(" ")).toContain("Busy, retrying at");
  expect(result.notes.join(" ")).toContain("Waiting to retry at");
});

describe("account link approval projection", () => {
  it("restores the initiator waiting state from the daemon snapshot", () => {
    const rows = accountView(view("signed_in", snapshot({ link_requests: [linkRequest()] }))).machines;
    const row = byKey(rows, macRow.installation_id);
    expect(row.status).toBe("linking");
    expect(row.detail).toBe("Waiting for approval on MacBook");
  });
  it("projects receiver approval on the requesting machine even with an older registry list", () => {
    const request = linkRequest({ incoming: true, notification_id: "notify-1", installation_id: "new-machine", name: "New machine" });
    const row = byKey(accountView(view("signed_in", snapshot({ link_requests: [request] }))).machines, "new-machine");
    expect(row.status).toBe("approval_requested");
    expect(row.controls.add).toBeNull();
    expect(row.linkRequest?.notification_id).toBe("notify-1");
  });
  it.each(["declined", "expired"] as const)("settles %s requests with a retry control", (decision) => {
    const state = snapshot({ link_requests: [linkRequest({ decision })] });
    const row = byKey(accountView(view("signed_in", state, new Set([macRow.installation_id]))).machines, macRow.installation_id);
    expect(row.status).toBe(decision === "expired" ? "link_expired" : "declined");
    expect(row.controls.add).toEqual({ reason: null });
    expect(settledLinks(new Set([macRow.installation_id]), state)).toEqual([macRow.installation_id]);
  });
});

it("uses a legacy pair's recorded adoption reason and never treats an untyped refusal as removal", () => {
  const result = accountView(view("signed_in", snapshot({
    enrollment: "refused", last_refusal: { reason: "forbidden" },
    pairs: [{ instance_id: "legacy-test", installation_id: null, admission: "legacy", online: true, name: "Legacy test", adoption_detail: "Waiting for the other machine to sign in" }],
  })));
  expect(result.removed).toBe(false);
  expect(result.saved[0].detail).toBe("Waiting for the other machine to sign in");
});
