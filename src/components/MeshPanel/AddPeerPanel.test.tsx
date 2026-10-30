import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import MyMachines, { type MyMachinesProps } from "./AddPeerPanel";
import { type AccountPresence } from "./accountMachines";
import { AccountMeshClientProvider } from "./accountMeshClient";
import { FakeAccountMeshClient, machine, macRow, NOW, pair, selfRow, snapshot, targetOf } from "./accountMachines.testdata";

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


it("requires a named confirmation before signing out another machine and allows cancel", async () => {
  const client = new FakeAccountMeshClient(snapshot());
  await mount(client, "signed_in");
  await click(button("Sign out MacBook"));
  expect(client.sessionRevokeCalls).toEqual([]);
  await click(button("Cancel"));
  expect(client.sessionRevokeCalls).toEqual([]);
  await click(button("Sign out MacBook"));
  await click(button("Confirm sign out"));
  expect(client.sessionRevokeCalls).toEqual([macRow.installation_id]);
  expect(client.revokeCalls).toEqual([]);
});
it("removes the exact selected installation only after confirmation", async () => {
  const client = new FakeAccountMeshClient(snapshot());
  await mount(client, "signed_in");
  await click(button("Remove MacBook from mesh"));
  expect(client.revokeCalls).toEqual([]);
  const checkbox = host.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
  expect(checkbox.checked).toBe(false);
  await click(button("Confirm remove"));
  expect(client.revokeCalls).toEqual([targetOf(macRow)]);
  expect(client.sessionRevokeCalls).toEqual([]);
});
it("keeps a stale installation removable and names green's sign-in cause", async () => {
  const green = machine("green", "green", { presence: "signed_out" });
  const client = new FakeAccountMeshClient(snapshot({ registry: { observed_at: NOW, rows: [selfRow, green] } }));
  await mount(client, "signed_in");
  expect(account("green")?.textContent).toContain("Sign in on green");
  expect(button("Remove green from mesh")).not.toBeNull();
  expect(button("Sign out green")).toBeNull();
});
it("connects through account state without Add, Approve or Disconnect controls", async () => {
  const client = new FakeAccountMeshClient(snapshot({ pairs: [pair("transport", "MacBook", { installation_id: macRow.installation_id })] }));
  await mount(client, "signed_in");
  expect(account("MacBook")?.textContent).toContain("Connected over your account mesh");
  expect([...host.querySelectorAll("button")].some(element => /^(Connect|Add|Approve|Decline|Disconnect)/.test(element.textContent ?? ""))).toBe(false);
});
it("uses the startup registry on mount and refreshes only on request", async () => {
  const client = new FakeAccountMeshClient(snapshot());
  await mount(client, "signed_in");
  expect(client.listCalls).toBe(0);
  await click(button("Refresh"));
  expect(client.listCalls).toBe(1);
  expect(client.enrollCalls).toBe(0);
});
it("updates registry names on backend events without another registry read", async () => {
  const client = new FakeAccountMeshClient(snapshot());
  await mount(client, "signed_in");
  await act(async () => client.emit(snapshot({ registry: { observed_at: NOW, rows: [selfRow, { ...macRow, display_name: "Travel Mac", metadata_revision: macRow.metadata_revision + 1 }] } })));
  expect(account("Travel Mac")).not.toBeNull();
  expect(client.listCalls).toBe(0);
});
it("renames the current installation using its exact metadata revision", async () => {
  const client = new FakeAccountMeshClient(snapshot());
  await mount(client, "signed_in");
  await click(button("Rename this machine"));
  await act(async () => type(host.querySelector<HTMLInputElement>('input[aria-label="Machine name"]')!, "Studio"));
  await click(button("Save"));
  expect(client.renameCalls).toEqual([[selfRow.metadata_revision, "Studio"]]);
});
it("offers sign-in without exposing old transport cards when signed out", async () => {
  const client = new FakeAccountMeshClient(snapshot({ pairs: [pair("old", "Old machine")] }));
  const spies = await mount(client, "signed_out");
  expect(host.textContent).not.toContain("Old machine");
  await click(button("Sign in"));
  expect(spies.onSignIn).toHaveBeenCalledTimes(1);
});
