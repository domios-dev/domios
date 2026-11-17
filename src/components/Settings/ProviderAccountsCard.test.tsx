import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ProviderDescriptor } from "../../lib/types";
import type { ProviderAccountsSnapshot } from "../../hooks/providerAccountsTypes";

const login = vi.hoisted(() => ({ render: vi.fn() }));
vi.mock("../Terminal/RuntimeLoginActions", () => ({ RuntimeLoginActions: (props: { onCheck?: () => void }) => {
  login.render(props);
  return <button type="button" onClick={props.onCheck}>Check scoped login</button>;
} }));
import { ProviderAccountsCard } from "./ProviderAccountsCard";

const provider = { id: "opaque-harness", display_name: "Fixture", icon_svg: "opaque-icon" } as ProviderDescriptor;
const initial = (): ProviderAccountsSnapshot => ({
  providerId: provider.id, revision: "opaque-revision", automaticSwitch: true, continuation: "proven", workspaceOverrides: [],
  accounts: [
    { id: "default", identity: "usual@example.test", status: "ready", tabsInUse: 0 },
    { id: "work", identity: "other@example.test", status: "ready", tabsInUse: 3 },
  ],
});
let host: HTMLDivElement;
let root: Root;
let snapshot: ProviderAccountsSnapshot;
const onAction = vi.fn();
const onRefresh = vi.fn();
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

beforeEach(() => {
  snapshot = initial();
  onAction.mockReset().mockResolvedValue(undefined);
  onRefresh.mockReset();
  login.render.mockReset();
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(async () => { await act(async () => root.unmount()); host.remove(); });

async function render(extra: Partial<Parameters<typeof ProviderAccountsCard>[0]> = {}) {
  await act(async () => root.render(<ProviderAccountsCard provider={provider} snapshot={snapshot} loading={false} error={null} onAction={onAction} onRefresh={onRefresh} {...extra} />));
}
function button(name: string, within: ParentNode = document.body): HTMLButtonElement {
  const found = Array.from(within.querySelectorAll<HTMLButtonElement>("button")).find(node => node.getAttribute("aria-label") === name || node.textContent?.startsWith(name));
  if (!found) throw new Error(`Missing button: ${name}`);
  return found;
}
async function click(name: string, within: ParentNode = document.body) {
  await act(async () => button(name, within).click());
}
function row(id: string): HTMLElement {
  const found = Array.from(host.querySelectorAll<HTMLElement>("[data-account-id]")).find(node => node.dataset.accountId === id);
  if (!found) throw new Error(`Missing account: ${id}`);
  return found;
}
async function menu(id: string) {
  await act(async () => button(`Actions for ${snapshot.accounts.find(account => account.id === id)!.identity}`, row(id))
    .dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, cancelable: true })));
  const found = document.querySelector<HTMLElement>('[role="menu"]:not([hidden])');
  if (!found) throw new Error("Account menu did not open");
  return found;
}
async function input(value: string) {
  const field = host.querySelector<HTMLInputElement>('input[aria-label="Account name"]')!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(field, value);
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

it("renders roster order and dispatches first-run sign-in and add without inventing an account", async () => {
  snapshot.accounts = [{ id: "default", status: "signedOut", tabsInUse: 0 }];
  await render();
  expect(host.querySelector('[role="list"]')).not.toBeNull();
  expect(row("default").getAttribute("aria-label")).toContain("Signed out");
  expect(host.querySelector('[role="switch"]')).toBeNull();
  await click("Sign in");
  expect(onAction).toHaveBeenLastCalledWith({ action: "signIn", accountId: "default" });
  await click("Add account");
  expect(onAction).toHaveBeenLastCalledWith({ action: "add" });
  expect(host.querySelectorAll("[data-account-id]")).toHaveLength(1);
});

it("keeps boundary moves and usual-home removal visible and inert", async () => {
  await render();
  const actions = await menu("default");
  expect(button("Move to top", actions).disabled).toBe(true);
  expect(button("Move up", actions).disabled).toBe(true);
  const remove = button("Sign out and remove", actions);
  expect(remove.disabled).toBe(true);
  expect(document.getElementById(remove.getAttribute("aria-describedby")!)?.textContent).toContain(provider.display_name);
  await act(async () => remove.click());
  expect(onAction).not.toHaveBeenCalled();
  await click("Move down", actions);
  expect(onAction).toHaveBeenCalledExactlyOnceWith({ action: "reorder", accountId: "default", position: 1 });
});

it("reorders by Alt+Arrow and applies unique accepted feedback with row focus", async () => {
  snapshot.feedback = { id: "old", kind: "moved", accountId: "default", position: 0 };
  await render();
  expect(host.querySelector('[aria-atomic="true"]')?.textContent).toBe("");
  row("work").focus();
  await act(async () => row("work").dispatchEvent(new KeyboardEvent("keydown", { code: "ArrowUp", altKey: true, bubbles: true })));
  expect(onAction).toHaveBeenCalledExactlyOnceWith({ action: "reorder", accountId: "work", position: 0 });
  snapshot = { ...snapshot, accounts: [...snapshot.accounts].reverse(), feedback: { id: "accepted", kind: "moved", accountId: "work", position: 0 } };
  await render();
  expect([...host.querySelectorAll<HTMLElement>("[data-account-id]")].map(node => node.dataset.accountId)).toEqual(["work", "default"]);
  expect(document.activeElement).toBe(row("work"));
  expect(host.querySelector('[aria-atomic="true"]')?.textContent).toContain("position 1");
  button("Add account").focus();
  snapshot = { ...snapshot, revision: "fresh" };
  await render();
  expect(document.activeElement).toBe(button("Add account"));
});

it("confirms removal inline, starts on Cancel, and restores menu focus on Escape", async () => {
  await render();
  await click("Sign out and remove", await menu("work"));
  expect(onAction).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(button("Cancel", row("work")));
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  await act(async () => button("Cancel", row("work")).dispatchEvent(new KeyboardEvent("keydown", { code: "Escape", bubbles: true })));
  expect(document.activeElement).toBe(button("Actions for other@example.test", row("work")));
  await click("Sign out and remove", await menu("work"));
  await click("Remove", row("work"));
  expect(onAction).toHaveBeenCalledExactlyOnceWith({ action: "remove", accountId: "work" });
});

it("bounds inline rename and preserves the draft on an unsuccessful mutation", async () => {
  await render();
  await click("Rename", await menu("work"));
  expect(document.activeElement).toBe(host.querySelector("input"));
  await input("x".repeat(81));
  expect(button("Save").disabled).toBe(true);
  await input("bad\u0001name");
  expect(button("Save").disabled).toBe(true);
  await input("  New name  ");
  onAction.mockRejectedValueOnce(new Error("hook owns error"));
  await act(async () => host.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })));
  expect(onAction).toHaveBeenCalledExactlyOnceWith({ action: "rename", accountId: "work", name: "New name" });
  expect(host.querySelector<HTMLInputElement>("input")?.value).toBe("  New name  ");
  await click("Cancel", row("work"));
  expect(document.activeElement).toBe(button("Actions for other@example.test"));
});

it.each(["unverified", "unsupported"] as const)("keeps %s switching off with an accessible reason", async continuation => {
  snapshot.continuation = continuation;
  await render();
  const toggle = host.querySelector<HTMLButtonElement>('[role="switch"]')!;
  expect(toggle.disabled).toBe(true);
  expect(toggle.getAttribute("aria-disabled")).toBe("true");
  expect(toggle.getAttribute("aria-checked")).toBe("false");
  expect(document.getElementById(toggle.getAttribute("aria-describedby")!)?.textContent).toContain(provider.display_name);
  await act(async () => toggle.click());
  expect(onAction).not.toHaveBeenCalled();
});

it("requests switching policy and workspace reset through the owner's handlers", async () => {
  snapshot.workspaceOverrides = [{ workspaceId: "opaque-workspace", workspaceName: "Project", accountId: "work" }];
  await render();
  await act(async () => host.querySelector<HTMLButtonElement>('[role="switch"]')!.click());
  expect(onAction).toHaveBeenLastCalledWith({ action: "automaticSwitch", enabled: false });
  await click("Reset account for Project");
  expect(onAction).toHaveBeenLastCalledWith({ action: "resetWorkspace", workspaceId: "opaque-workspace" });
});

it("passes the exact new-account login context, then focuses only an accepted added row", async () => {
  const context = { tabId: "new-account-tab", state: { state: "loginPending" as const, epoch: 19 } };
  snapshot.adding = { context, status: "signingIn" };
  await render();
  expect(login.render).toHaveBeenLastCalledWith(expect.objectContaining({ providerId: provider.id, tabId: context.tabId, state: context.state, variant: "inline" }));
  expect(button("Add account").disabled).toBe(true);
  await click("Check scoped login");
  expect(onRefresh).toHaveBeenCalledTimes(1);
  await click("Cancel");
  expect(onAction).toHaveBeenLastCalledWith({ action: "cancelAdd" });
  snapshot = { ...snapshot, adding: undefined, accounts: [...snapshot.accounts, { id: "added", identity: "new@example.test", status: "ready", tabsInUse: 0 }],
    feedback: { id: "added-once", kind: "added", accountId: "added", position: 2 } };
  await render();
  expect(document.activeElement).toBe(row("added"));
  expect(host.querySelector('[aria-atomic="true"]')?.textContent).toContain("new@example.test");
});

it("routes duplicate and failed additions to retry without adding a row or exposing details inline", async () => {
  const context = { tabId: "isolated", state: { state: "loginPending" as const, epoch: 2 } };
  snapshot.adding = { context, status: "duplicate", identity: "other@example.test" };
  await render();
  expect(login.render).not.toHaveBeenCalled();
  await click("Use a different account");
  expect(onAction).toHaveBeenLastCalledWith({ action: "retryAdd" });
  snapshot = { ...snapshot, adding: { context, status: "failed", details: "opaque diagnostic" } };
  await render();
  const details = host.querySelector("details")!;
  expect(details.open).toBe(false);
  expect(details.textContent).toContain("opaque diagnostic");
  await click("Try again");
  expect(onAction).toHaveBeenLastCalledWith({ action: "retryAdd" });
  expect(host.querySelectorAll("[data-account-id]")).toHaveLength(2);
});

it("names exhausted and removing rows and wires only the row's sign-in context", async () => {
  snapshot.accounts[0] = { ...snapshot.accounts[0], status: "exhausted", resetsAtMs: 2_000_000_000_000 };
  snapshot.accounts[1] = { ...snapshot.accounts[1], status: "signingIn", login: { tabId: "work-login", state: { state: "loginPending", epoch: 7 } } };
  await render();
  expect(row("default").getAttribute("aria-label")).toContain("Limit reached, resets");
  expect(login.render).toHaveBeenLastCalledWith(expect.objectContaining({ tabId: "work-login", state: snapshot.accounts[1].login!.state }));
  await click("Cancel", row("work"));
  expect(onAction).toHaveBeenLastCalledWith({ action: "cancelSignIn", accountId: "work" });
  snapshot = { ...snapshot, accounts: [snapshot.accounts[0], { ...snapshot.accounts[1], status: "removing", tabsToFinish: 2 }] };
  await render();
  expect(row("work").getAttribute("aria-label")).toContain("Removing after 2 tabs");
  expect(button("Actions for other@example.test", row("work")).disabled).toBe(true);
  onAction.mockClear();
  await act(async () => row("work").dispatchEvent(new KeyboardEvent("keydown", { code: "ArrowUp", altKey: true, bubbles: true })));
  expect(onAction).not.toHaveBeenCalled();
});

it("guards repeated actions while pending and leaves errors owned by the hook", async () => {
  let reject!: (error: Error) => void;
  onAction.mockImplementation(() => new Promise<void>((_resolve, fail) => { reject = fail; }));
  await render();
  await act(async () => { button("Add account").click(); button("Add account").click(); });
  expect(onAction).toHaveBeenCalledExactlyOnceWith({ action: "add" });
  expect(button("Add account").disabled).toBe(true);
  await act(async () => reject(new Error("opaque hook failure")));
  await render({ error: "opaque hook failure" });
  expect(button("Add account").disabled).toBe(false);
  await click("Try again");
  expect(onRefresh).toHaveBeenCalledTimes(1);
});

it("does not show a foreign snapshot while the matching provider is loading", async () => {
  await render({ snapshot: { ...snapshot, providerId: "foreign" }, loading: true });
  expect(host.querySelectorAll("[data-account-id]")).toHaveLength(0);
  expect(host.querySelector('[role="status"][aria-label="Loading accounts"]')).not.toBeNull();
});
