import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import MyMachines, { type MyMachinesProps } from "./AddPeerPanel";
import { SIGN_IN_COPY, SIGNED_OUT_COPY, type AccountPresence } from "./accountMachines";
import { AccountMeshClientProvider, type AccountMeshSnapshot } from "./accountMeshClient";
import { AccountMeshRefusalError } from "../billing/accountMeshLimit";
import { freeLimitGate } from "../billing/freeLimitGate";
import { airRow, deferred, FakeAccountMeshClient, lenovoRow, machine, macRow, NOW, pair, selfRow, snapshot, targetOf } from "./accountMachines.testdata";

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

function handlers() {
  return { onSignIn: vi.fn(), onFailure: vi.fn(), onChanged: vi.fn() };
}

async function mount(client: FakeAccountMeshClient, account: AccountPresence, props: Partial<MyMachinesProps> = {}) {
  const spies = handlers();
  await act(async () => root.render(
    <AccountMeshClientProvider value={client}>
      <MyMachines account={account} now={() => NOW} {...spies} {...props} />
    </AccountMeshClientProvider>,
  ));
  return spies;
}

const button = (name: string) => host.querySelector<HTMLButtonElement>(`button[aria-label="${name}"]`)
  ?? [...host.querySelectorAll<HTMLButtonElement>("button")].find((candidate) => candidate.textContent?.trim() === name && !candidate.hasAttribute("aria-label"))
  ?? null;
const row = (list: string, name: string) => host.querySelector<HTMLLIElement>(`ul[aria-label="${list}"] li[aria-label="${name}"]`);
const account = (name: string) => row("Your account's machines", name);
const click = async (target: HTMLElement | null) => {
  expect(target).not.toBeNull();
  await act(async () => target!.click());
};
function type(input: HTMLInputElement, value: string) {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

describe("My machines sign-in surface", () => {
  it("asks to sign in and starts the account sign-in flow", async () => {
    const client = new FakeAccountMeshClient(snapshot({ binding: "none", self: null, registry: null }));
    const spies = await mount(client, "signed_out");
    expect(host.querySelector('[role="status"]')?.textContent).toBe(SIGN_IN_COPY);
    expect(host.querySelector('button[aria-label^="Connect "]')).toBeNull();
    await click(button("Sign in"));
    expect(spies.onSignIn).toHaveBeenCalledTimes(1);
  });

  it("refreshes into My machines after sign-in without a reload", async () => {
    const client = new FakeAccountMeshClient(snapshot({ binding: "none", self: null, registry: null }));
    function Harness() {
      const [presence, setPresence] = useState<AccountPresence>("signed_out");
      return (
        <AccountMeshClientProvider value={client}>
          <MyMachines
            account={presence}
            now={() => NOW}
            onSignIn={() => { client.state = snapshot(); setPresence("signed_in"); }}
            onFailure={() => {}}
          />
        </AccountMeshClientProvider>
      );
    }
    await act(async () => root.render(<Harness />));
    await click(button("Sign in"));
    expect(account("MacBook")?.getAttribute("data-status")).toBe("unlinked");
    expect(host.querySelector('[role="group"][aria-label="This machine: Studio PC"]')).not.toBeNull();
  });

  it("keeps saved pairings listed as blocked after explicit sign-out", async () => {
    const client = new FakeAccountMeshClient(snapshot({
      traffic: "blocked_signed_out",
      pairs: [pair("mesh-mac", "MacBook", { installation_id: macRow.installation_id }), pair("mesh-old", "Old laptop", { admission: "legacy" })],
    }));
    await mount(client, "signed_out");
    expect(host.querySelector('[role="status"]')?.textContent).toBe(SIGNED_OUT_COPY);
    for (const name of ["MacBook", "Old laptop"]) {
      const saved = row("Saved pairings", name);
      expect(saved?.getAttribute("data-status")).toBe("blocked");
      expect(saved?.textContent).toContain(SIGNED_OUT_COPY);
      expect(saved?.querySelector("button")).toBeNull();
    }
  });
});

describe("My machines Add", () => {
  it("shows a refused enrollment and exposes retry action", async () => {
    const client = new FakeAccountMeshClient(snapshot({
      self: null,
      enrollment: "refused",
      last_refusal: { reason: "verification-unavailable" },
      registry: null,
    }));
    await mount(client, "signed_in");

    expect(host.textContent).toContain("Account enrollment was refused.");
    expect(host.textContent).toContain("Machine membership could not be verified right now.");
    await click(button("Retry enrollment"));
    expect(client.enrollCalls).toBe(1);
  });

  it("shows the typed list failure and retry control instead of the empty state", async () => {
    const client = new FakeAccountMeshClient(snapshot({ registry: null }));
    client.listOutcome = { ok: false, failure: { reason: "unauthenticated" } };
    await mount(client, "signed_in");

    expect(host.textContent).toContain("Your account's machines could not be listed.");
    expect(host.textContent).toContain("Your account session could not be verified. Sign in again.");
    expect(host.textContent).not.toContain("No other machines in your account yet.");
    expect(button("Refresh My machines")).not.toBeNull();
  });

  it("loads the registry on mount and Refresh before showing remote Add rows", async () => {
    const client = new FakeAccountMeshClient(snapshot({ registry: null }));
    client.listRows = [selfRow, macRow];

    await mount(client, "signed_in");

    expect(client.listCalls).toBe(1);
    expect(account("MacBook")?.getAttribute("data-status")).toBe("unlinked");
    expect(button("Connect MacBook")).not.toBeNull();

    await click(button("Refresh My machines"));
    expect(client.listCalls).toBe(2);
    expect(account("MacBook")?.getAttribute("data-status")).toBe("unlinked");
  });

  it("links one unlinked machine and shows linked only after the handshake is observed", async () => {
    const client = new FakeAccountMeshClient(snapshot());
    const spies = await mount(client, "signed_in");
    expect(account("MacBook")?.getAttribute("data-status")).toBe("unlinked");
    expect(account("Lenovo")?.getAttribute("data-status")).toBe("unlinked");
    expect(host.querySelector('input[placeholder*=":"]')).toBeNull();

    await click(button("Connect MacBook"));
    expect(client.linkCalls).toEqual([targetOf(macRow)]);
    expect(account("MacBook")?.getAttribute("data-status")).toBe("linking");
    expect(button("Connect MacBook")?.disabled).toBe(true);
    expect(button("Connect MacBook")?.getAttribute("aria-busy")).toBe("true");
    expect(button("Connect Lenovo")?.disabled).toBe(false);

    await act(async () => client.linkOutcome.resolve());
    expect(account("MacBook")?.getAttribute("data-status")).toBe("linking");

    await act(async () => client.emit(snapshot({ pairs: [pair("mesh-mac", "mac.local", { installation_id: macRow.installation_id })] })));
    expect(account("MacBook")?.getAttribute("data-status")).toBe("linked");
    expect(button("Connect MacBook")).toBeNull();
    expect(button("Disconnect MacBook")).not.toBeNull();
    expect(spies.onFailure).not.toHaveBeenCalled();
  });

  it("disables Add with a reason when the machine has no reachable endpoint", async () => {
    const client = new FakeAccountMeshClient(snapshot({ registry: { observed_at: NOW, rows: [selfRow, airRow] } }));
    await mount(client, "signed_in");
    const add = button("Connect Air");
    expect(add?.disabled).toBe(true);
    const reason = host.querySelector(`#${CSS.escape(add!.getAttribute("aria-describedby")!)}`);
    expect(reason?.textContent).toBeTruthy();
    expect(account("Air")?.getAttribute("data-status")).toBe("endpoint_missing");
  });

  it("keeps the record and reports a failed link through the toast handler", async () => {
    const client = new FakeAccountMeshClient(snapshot());
    const spies = await mount(client, "signed_in");
    await click(button("Connect MacBook"));
    await act(async () => client.linkOutcome.reject(new AccountMeshRefusalError({ requestId: "req-unreachable", refusal: { reason: "peer-unreachable" } })));
    expect(spies.onFailure).toHaveBeenCalledWith("connect MacBook", { reason: "peer-unreachable" });
    expect(freeLimitGate.current()).toBeNull();
    expect(account("MacBook")?.getAttribute("data-status")).toBe("unlinked");
    expect(button("Connect MacBook")?.disabled).toBe(false);

    client.linkOutcome = deferred<void>();
    await click(button("Connect MacBook"));
    await act(async () => client.linkOutcome.reject(new Error("transport closed")));
    expect(spies.onFailure).toHaveBeenLastCalledWith("connect MacBook", { reason: "unavailable" });
    expect(account("MacBook")?.getAttribute("data-status")).toBe("unlinked");
  });

  it("opens the plans modal for the server's machine-limit refusal instead of a toast", async () => {
    const client = new FakeAccountMeshClient(snapshot());
    const spies = await mount(client, "signed_in");
    await click(button("Connect Lenovo"));
    const refusal = { reason: "mesh.machines" as const, limit: 2, active_count: 2 };
    try {
      await act(async () => client.linkOutcome.reject(new AccountMeshRefusalError({ requestId: "req-limit", refusal })));
      expect(freeLimitGate.current()).toEqual({ key: "mesh.machines", requestId: "req-limit", limit: 2, activeCount: 2 });
      expect(spies.onFailure).not.toHaveBeenCalled();
      expect(account("Lenovo")?.getAttribute("data-status")).toBe("unlinked");
      expect(button("Connect Lenovo")?.disabled).toBe(false);
    } finally {
      freeLimitGate.close();
    }
  });

  it("refreshes from change events, not timers", async () => {
    const client = new FakeAccountMeshClient(snapshot());
    await mount(client, "signed_in");
    const before = client.snapshotCalls;
    await act(async () => client.emit(snapshot({ registry: { observed_at: NOW, rows: [selfRow, macRow] } })));
    expect(client.snapshotCalls).toBe(before + 1);
    expect(account("Lenovo")).toBeNull();
  });
});

describe("My machines management", () => {
  it("renames this installation with its current metadata revision and keeps the draft on failure", async () => {
    const client = new FakeAccountMeshClient(snapshot());
    client.renameOutcomes = [{ ok: false, failure: { reason: "conflict" } }, { ok: true }];
    const spies = await mount(client, "signed_in");
    expect(host.querySelector('button[aria-label^="Rename MacBook"]')).toBeNull();
    await click(button("Rename this machine"));
    const input = host.querySelector<HTMLInputElement>('input[aria-label="Machine name"]')!;
    await act(async () => type(input, "Workstation"));
    await click(button("Save"));
    expect(client.renameCalls).toEqual([[2, "Workstation"]]);
    expect(spies.onFailure).toHaveBeenCalledWith("rename this machine", { reason: "conflict" });
    const kept = host.querySelector<HTMLInputElement>('input[aria-label="Machine name"]');
    expect(kept?.value).toBe("Workstation");
    expect(document.activeElement).toBe(kept);

    await click(button("Save"));
    expect(client.renameCalls).toEqual([[2, "Workstation"], [2, "Workstation"]]);
    expect(host.querySelector('input[aria-label="Machine name"]')).toBeNull();
    expect(document.activeElement).toBe(button("Rename this machine"));
  });

  it("revokes the selected machine with its exact revision after confirmation; cancel and failure keep it", async () => {
    const client = new FakeAccountMeshClient(snapshot());
    client.revokeOutcomes = [{ ok: false, failure: { reason: "conflict" } }, { ok: true }];
    const spies = await mount(client, "signed_in");

    await click(button("Revoke Lenovo"));
    expect(host.querySelector('[role="group"][aria-label="Confirm revoke Lenovo"]')).not.toBeNull();
    await click([...host.querySelectorAll<HTMLButtonElement>('[aria-label="Confirm revoke Lenovo"] button')].find((b) => b.textContent === "Cancel")!);
    expect(client.revokeCalls).toEqual([]);
    expect(document.activeElement).toBe(button("Revoke Lenovo"));

    await click(button("Revoke Lenovo"));
    const confirm = () => [...host.querySelectorAll<HTMLButtonElement>('[aria-label="Confirm revoke Lenovo"] button')].find((b) => b.textContent === "Revoke")!;
    await click(confirm());
    expect(client.revokeCalls).toEqual([targetOf(lenovoRow)]);
    expect(spies.onFailure).toHaveBeenCalledWith("revoke Lenovo", { reason: "conflict" });
    expect(account("Lenovo")).not.toBeNull();
    expect(document.activeElement).toBe(confirm());

    await click(confirm());
    expect(client.revokeCalls).toEqual([targetOf(lenovoRow), targetOf(lenovoRow)]);
    expect(account("Lenovo")).not.toBeNull();
    expect(host.querySelector('[aria-label="Confirm revoke Lenovo"]')).toBeNull();
    await act(async () => client.emit(snapshot({ registry: { observed_at: NOW, rows: [selfRow, macRow] } })));
    expect(account("Lenovo")).toBeNull();
  });

  it("unpairs a legacy pairing locally without revoking an account slot", async () => {
    const client = new FakeAccountMeshClient(snapshot({ pairs: [pair("mesh-old", "Old laptop", { admission: "legacy", online: true })] }));
    const spies = await mount(client, "signed_in");
    const legacy = row("Saved pairings", "Old laptop");
    expect(legacy?.getAttribute("data-status")).toBe("legacy");
    expect(button("Revoke Old laptop")).toBeNull();
    await click(button("Disconnect Old laptop"));
    await click([...host.querySelectorAll<HTMLButtonElement>('[aria-label="Confirm disconnect Old laptop"] button')].find((b) => b.textContent === "Disconnect")!);
    expect(client.unpairCalls).toEqual(["mesh-old"]);
    expect(client.revokeCalls).toEqual([]);
    expect(spies.onChanged).toHaveBeenCalled();
    expect(row("Saved pairings", "Old laptop")).not.toBeNull();
  });

  it("keeps the pairing when local unpair fails", async () => {
    const client = new FakeAccountMeshClient(snapshot({ pairs: [pair("mesh-mac", "MacBook", { installation_id: macRow.installation_id })] }));
    client.unpairFailure = new Error("daemon unavailable");
    const spies = await mount(client, "signed_in");
    await click(button("Disconnect MacBook"));
    await click([...host.querySelectorAll<HTMLButtonElement>('[aria-label="Confirm disconnect MacBook"] button')].find((b) => b.textContent === "Disconnect")!);
    expect(spies.onFailure).toHaveBeenCalledWith("disconnect MacBook", { reason: "unavailable" });
    expect(account("MacBook")?.getAttribute("data-status")).toBe("linked");
    expect(host.querySelector('[aria-label="Confirm disconnect MacBook"]')).not.toBeNull();
  });

  it("does not offer any control for a pending sign-in", async () => {
    const client = new FakeAccountMeshClient(snapshot());
    const gate = deferred<AccountMeshSnapshot>();
    client.snapshot = () => gate.promise;
    await mount(client, "pending");
    expect(host.querySelector("button")).toBeNull();
    expect(host.querySelector('section[aria-label="My machines"]')?.getAttribute("aria-busy")).toBe("true");
  });
});

const lockBadges = (row: Element | null | undefined) => Array.from(row?.querySelectorAll("[role=img]") ?? []).filter((mark) => mark.getAttribute("aria-label") === "Locked: free plan limit").length;

describe("My machines over the plan's machine limit", () => {
  afterEach(() => act(() => freeLimitGate.close()));

  it("opens the plans modal from an over-limit row and keeps the row listed", async () => {
    const limitRow = machine("inst-limit-b", "Limit", { eligibility: "mesh-allowance" });
    const client = new FakeAccountMeshClient(snapshot({
      registry: { observed_at: NOW, rows: [selfRow, macRow, limitRow] },
      last_refusal: { reason: "mesh.machines", limit: 2, active_count: 3 },
    }));
    await mount(client, "signed_in");
    await click(button("See plans for Limit"));
    expect(freeLimitGate.current()).toMatchObject({ key: "mesh.machines", limit: 2, activeCount: 3 });
    expect(account("Limit")?.dataset.status).toBe("over_limit");
    expect(lockBadges(account("Limit"))).toBe(1);
    expect(lockBadges(account("Mac"))).toBe(0);
  });

  it("keeps a locked machine's legacy pairing listed with a plans control", async () => {
    const client = new FakeAccountMeshClient(snapshot({
      traffic: "plan_limit_locked",
      pairs: [pair("peer-legacy", "Old box", { admission: "legacy" })],
    }));
    await mount(client, "signed_in");
    const saved = row("Saved pairings", "Old box");
    expect(saved?.dataset.status).toBe("over_limit");
    expect(lockBadges(saved)).toBe(1);
    await click(button("See plans for Old box"));
    expect(freeLimitGate.current()?.key).toBe("mesh.machines");
    expect(row("Saved pairings", "Old box")).not.toBeNull();
  });
});
