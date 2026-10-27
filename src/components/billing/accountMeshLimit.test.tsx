import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

const fetches = vi.hoisted(() => ({ localFetch: vi.fn(), localJson: vi.fn(async () => ({ consent: "off" })) }));
vi.mock("../../lib/webApi", async (actual) => ({ ...await actual<object>(), ...fetches }));

import { AccountMeshRefusalError, openOnMeshRefusal, raiseMeshMachineLimit } from "./accountMeshLimit";
import { FreeLimitPlansModal } from "./FreeLimitPlansModal";
import { freeLimitGate, raiseFreeLimit } from "./freeLimitGate";
import { FREE_LIMIT_REACHED } from "./freeLimitModel";
import { AccountMeshClientProvider } from "../MeshPanel/accountMeshClient";
import { FakeAccountMeshClient, macRow, lenovoRow, NOW, snapshot } from "../MeshPanel/accountMachines.testdata";
import { useMyMachines } from "../MeshPanel/useMyMachines";
import { MachineLimitPaywall } from "../MeshPanel/MachineLimitPaywall";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const machineLimit = (activeCount = 2, limit = 2) => ({ reason: "mesh.machines", limit, active_count: activeCount });

let root: Root | null = null;
let nextRequest = 0;
const requestId = () => `opaque-request-${++nextRequest}`;

const json = (body: unknown) => new Response(JSON.stringify(body), { status: 200 });
const signedInDaemon = async (url: string) => {
  const path = url.split("?")[0];
  if (path.endsWith("/status")) return json({ identity: { uid: "user", display_name: "Ada", email: "ada@example.test", provider: "fake" }, pending: false, last_sign_in_failure: null });
  if (path.includes("/plan")) return json({ entitlements: { plan: "free", status: "active", current_period_end: 0, cancel_at_period_end: false },
    access: { authority: "free", full_access: false }, stale: false });
  return json({});
};

async function mountModal(upgrade: (planKey: string) => Promise<void> = async () => {}) {
  fetches.localFetch.mockImplementation(signedInDaemon);
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<FreeLimitPlansModal upgrade={upgrade} />));
}

const dialog = () => document.querySelector<HTMLElement>("[role=dialog]");
const prompt = () => freeLimitGate.current();
const button = (name: string) => [...document.querySelectorAll("button")].find((candidate) => candidate.textContent === name)!;
const settle = () => act(async () => {});
const click = async (name: string) => {
  await act(async () => { button(name).click(); });
  await settle();
};
const deliver = (id: string, refusal: unknown) => act(async () => { raiseMeshMachineLimit({ requestId: id, refusal }); });

/** Stands in for the account-Mesh facade: admits by the server's answer and never writes a record it refused. */
function fakeMachines(answer: () => unknown) {
  const records = ["self-paired-relationship", "second-paired-relationship"];
  const attempts: string[] = [];
  const add = async (target: string) => {
    attempts.push(target);
    const refusal = answer();
    if (refusal instanceof Error) throw refusal;
    if (refusal) throw new AccountMeshRefusalError({ requestId: requestId(), refusal });
    records.push(target);
    return target;
  };
  return { records, attempts, add };
}

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  act(() => freeLimitGate.close());
  freeLimitGate.setLimits(null);
  document.body.replaceChildren();
});

describe("automatic enroll after sign-in or restore", () => {
  it("opens the existing plans dialog from the refused machine's See plans control", async () => {
    const client = new FakeAccountMeshClient(snapshot({ self: null, traffic: "plan_limit_locked", enrollment: "refused", last_refusal: machineLimit() as { reason: "mesh.machines"; limit: number; active_count: number }, registry: { observed_at: NOW, rows: [macRow, lenovoRow] } }));
    const onSignIn = () => {};
    function RefusedMachine() {
      const machines = useMyMachines({ account: "signed_in", onSignIn });
      return <MachineLimitPaywall machines={machines} />;
    }
    await mountModal();
    await act(async () => root!.render(<AccountMeshClientProvider value={client}><RefusedMachine /><FreeLimitPlansModal /></AccountMeshClientProvider>));
    expect(dialog()).toBeNull();
    await click("See plans");
    expect(prompt()).toMatchObject({ key: "mesh.machines", limit: 2, activeCount: 2 });
    await click("Not now");
    expect(dialog()).toBeNull();
    expect(client.revokeCalls).toEqual([]);
    expect(client.enrollCalls).toBe(0);
  });

  it("a refusal delivered before any Mesh view mounts opens the modal once mounted", async () => {
    raiseMeshMachineLimit({ requestId: requestId(), refusal: machineLimit() });
    await mountModal();
    expect(prompt()).toMatchObject({ key: "mesh.machines", limit: 2, activeCount: 2 });
  });

  it("a refusal delivered after mount opens it with the server's own maximum", async () => {
    await mountModal();
    await deliver(requestId(), machineLimit(5, 5));
    expect(prompt()).toMatchObject({ key: "mesh.machines", limit: 5, activeCount: 5 });
  });

  it("the same request replayed by a refresh or remount never reopens or overrides dismissal", async () => {
    const id = requestId();
    await mountModal();
    await deliver(id, machineLimit());
    await deliver(id, machineLimit());
    expect(prompt()).toMatchObject({ key: "mesh.machines", limit: 2, activeCount: 2 });
    await click("Not now");
    await deliver(id, machineLimit());
    await act(async () => root!.unmount());
    await mountModal();
    await deliver(id, machineLimit());
    expect(dialog()).toBeNull();
    await deliver(requestId(), machineLimit());
    expect(prompt()).toMatchObject({ key: "mesh.machines", limit: 2, activeCount: 2 });
  });

  it("a paid or dormant local plan payload neither suppresses nor fakes a server machine refusal", async () => {
    await mountModal();
    freeLimitGate.setLimits({ enforcing: false, max: { workspaces: null, teams: null, "agent.tabs": null } });
    await deliver(requestId(), machineLimit());
    expect(prompt()).toMatchObject({ key: "mesh.machines", limit: 2, activeCount: 2 });
    await click("Not now");
    freeLimitGate.setLimits({ enforcing: true, max: { workspaces: 2, teams: 6, "agent.tabs": 35 } });
    await deliver(requestId(), { reason: "unavailable" });
    expect(dialog()).toBeNull();
  });
});

describe("explicit Add or retry", () => {
  it("a machine-limit refusal opens the modal, creates no pair and keeps every record", async () => {
    await mountModal();
    const machines = fakeMachines(() => machineLimit());
    let result: unknown = "unset";
    await act(async () => { result = await openOnMeshRefusal(() => machines.add("third-machine")); });
    expect(result).toBeUndefined();
    expect(prompt()).toMatchObject({ key: "mesh.machines", limit: 2, activeCount: 2 });
    expect(machines.records).toEqual(["self-paired-relationship", "second-paired-relationship"]);
  });

  it("each retry is its own request and opens the modal again", async () => {
    await mountModal();
    const machines = fakeMachines(() => machineLimit());
    await act(async () => { await openOnMeshRefusal(() => machines.add("third-machine")); });
    await click("Not now");
    await act(async () => { await openOnMeshRefusal(() => machines.add("third-machine")); });
    expect(prompt()).toMatchObject({ key: "mesh.machines", limit: 2, activeCount: 2 });
    expect(machines.attempts).toHaveLength(2);
  });

  it("an admitted Add resolves and never opens plans", async () => {
    await mountModal();
    const machines = fakeMachines(() => null);
    await act(async () => { await openOnMeshRefusal(() => machines.add("second-machine")); });
    expect(dialog()).toBeNull();
    expect(machines.records).toContain("second-machine");
  });

  it("Upgrade checks out pro_yearly and grants nothing locally: no retry, no record, no reopen", async () => {
    const upgrade = vi.fn(async () => {});
    await mountModal(upgrade);
    const machines = fakeMachines(() => machineLimit());
    await act(async () => { await openOnMeshRefusal(() => machines.add("third-machine")); });
    await click("Upgrade");
    expect(upgrade).toHaveBeenCalledWith("pro_yearly");
    expect(dialog()).toBeNull();
    expect(machines.attempts).toHaveLength(1);
    expect(machines.records).not.toContain("third-machine");
  });
});

describe("failures that are not the server's machine limit never open plans", () => {
  const structured: Array<[string, unknown]> = [
    ["unauthenticated", { reason: "unauthenticated" }],
    ["app check", { reason: "app-check" }],
    ["foreign or forbidden", { reason: "forbidden" }],
    ["invalid", { reason: "invalid" }],
    ["conflict", { reason: "conflict" }],
    ["rate limited 429", { reason: "rate-limited", retry_after_seconds: 30 }],
    ["unknown authority", { reason: "authority-unavailable" }],
    ["unavailable 503", { reason: "unavailable" }],
    ["mesh allowance 403", { reason: "mesh-allowance" }],
    ["revoked", { reason: "revoked" }],
    ["expired", { reason: "expired" }],
    ["limit without a server count", { reason: "mesh.machines", limit: 2 }],
    ["count below the limit", machineLimit(1)],
    ["zero limit", machineLimit(0, 0)],
    ["bare reason text", "mesh.machines"],
  ];

  it.each(structured)("%s through the facade", async (_name, refusal) => {
    await mountModal();
    const error = new AccountMeshRefusalError({ requestId: requestId(), refusal });
    await act(async () => { await expect(openOnMeshRefusal(async () => { throw error; })).rejects.toBe(error); });
    await deliver(requestId(), refusal);
    expect(dialog()).toBeNull();
  });

  it.each([
    ["offline", new TypeError("Failed to fetch")],
    ["message naming the limit", new Error("mesh.machines limit reached (limit: 2)")],
    ["daemon text marker naming the limit", new Error(`${FREE_LIMIT_REACHED}: opaque (limit: mesh.machines)`)],
    ["untyped server-shaped object", machineLimit()],
  ])("%s outside the typed refusal", async (_name, error) => {
    await mountModal();
    await act(async () => { await expect(openOnMeshRefusal(async () => { throw error; })).rejects.toBe(error); });
    expect(raiseFreeLimit(error, "workspaces")).toBe(false);
    expect(dialog()).toBeNull();
  });

  it("a machine refusal without request identity cannot be deduplicated and stays closed", async () => {
    await mountModal();
    await deliver("", machineLimit());
    expect(dialog()).toBeNull();
  });
});
