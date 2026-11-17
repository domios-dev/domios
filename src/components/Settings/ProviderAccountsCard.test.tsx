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
  expect(row("work").textContent).toContain("Its tabs continue on the next account.");
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

it.each(["checking", "probeFailed", "unknown"] as const)("keeps identity and reorder available for %s accounts", async status => {
  snapshot.accounts[1] = { ...snapshot.accounts[1], status };
  await render();
  expect(row("work").getAttribute("aria-label")).toContain("other@example.test");
  expect(Array.from(row("work").querySelectorAll("button")).some(node => node.textContent === "Sign in")).toBe(false);
  expect(button("Actions for other@example.test", row("work")).disabled).toBe(false);
  await act(async () => row("work").dispatchEvent(new KeyboardEvent("keydown", { code: "ArrowUp", altKey: true, bubbles: true })));
  expect(onAction).toHaveBeenCalledExactlyOnceWith({ action: "reorder", accountId: "work", position: 0 });
  onAction.mockClear();
  await click("Move to top", await menu("work"));
  expect(onAction).toHaveBeenCalledExactlyOnceWith({ action: "reorder", accountId: "work", position: 0 });
});

it("removes checking activity as soon as the bounded probe settles, retaining the cached identity", async () => {
  snapshot.accounts[1] = { ...snapshot.accounts[1], name: "Work", status: "checking" };
  await render();
  expect(row("work").getAttribute("aria-label")).toBe("Work, other@example.test, Checking sign-in");
  expect(row("work").querySelector('[role="status"][aria-label="Checking sign-in"]')).not.toBeNull();
  snapshot = { ...snapshot, accounts: [snapshot.accounts[0], { ...snapshot.accounts[1], status: "ready" }] };
  await render();
  expect(row("work").querySelector('[role="status"]')).toBeNull();
  expect(row("work").getAttribute("aria-label")).toContain("other@example.test");
});

it("rechecks a failed probe and exposes its reason only through an inert menu description", async () => {
  snapshot.accounts[1] = { ...snapshot.accounts[1], status: "probeFailed", probeFailureReason: "opaque probe reason" };
  await render();
  expect(row("work").getAttribute("aria-label")).toBe("other@example.test, Couldn't check sign-in");
  expect(row("work").textContent).not.toContain("opaque probe reason");
  await click("Check again", row("work"));
  expect(onAction).toHaveBeenCalledExactlyOnceWith({ action: "check", accountId: "work" });
  const actions = await menu("work");
  const reason = button("Couldn't check sign-in", actions);
  expect(reason.getAttribute("aria-disabled")).toBe("true");
  expect(document.getElementById(reason.getAttribute("aria-describedby")!)?.textContent).toBe("opaque probe reason");
  await act(async () => reason.click());
  expect(onAction).toHaveBeenCalledTimes(1);
});

it("does not infer readiness or signed-out state when identity reporting is unsupported", async () => {
  snapshot.accounts = [{ id: "default", status: "unknown", tabsInUse: 3, plan: "opaque plan" }];
  await render();
  expect(row("default").getAttribute("aria-label")).toBe("Your usual sign-in");
  expect(row("default").querySelector('[role="status"]')).toBeNull();
  expect(row("default").textContent).not.toContain("opaque plan");
  expect(row("default").textContent).not.toContain("Signed out");
  expect(row("default").querySelectorAll("button")).toHaveLength(1);
});

it.each([
  { continuation: "unverified", tabs: 1 }, { continuation: "unverified", tabs: 2 },
  { continuation: "unsupported", tabs: 1 }, { continuation: "unsupported", tabs: 2 },
] as const)("keeps removal focusable and inert for $tabs tabs with $continuation continuation", async ({ continuation, tabs }) => {
  snapshot.continuation = continuation;
  snapshot.accounts[1] = { ...snapshot.accounts[1], tabsInUse: tabs };
  await render();
  const actions = await menu("work");
  const remove = button("Sign out and remove", actions);
  expect(remove.disabled).toBe(false);
  expect(remove.getAttribute("aria-disabled")).toBe("true");
  expect(document.getElementById(remove.getAttribute("aria-describedby")!)?.textContent)
    .toBe(tabs === 1 ? "1 tab still uses it. Close them first." : "2 tabs still use it. Close them first.");
  remove.focus();
  expect(document.activeElement).toBe(remove);
  await act(async () => {
    remove.click();
    remove.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", code: "Enter", bubbles: true, cancelable: true }));
  });
  expect(onAction).not.toHaveBeenCalled();
  expect(row("work").querySelector('[role="status"]')).toBeNull();
  expect(Array.from(row("work").querySelectorAll("button")).some(node => node.textContent === "Remove")).toBe(false);
});

it.each(["unverified", "unsupported"] as const)("allows unused account removal without promising %s continuation", async continuation => {
  snapshot.continuation = continuation;
  snapshot.accounts[1] = { ...snapshot.accounts[1], tabsInUse: 0 };
  await render();
  await click("Sign out and remove", await menu("work"));
  expect(row("work").textContent).not.toContain("Its tabs continue");
  expect(document.activeElement).toBe(button("Cancel", row("work")));
  await click("Remove", row("work"));
  expect(onAction).toHaveBeenCalledExactlyOnceWith({ action: "remove", accountId: "work" });
});

it("withdraws an open removal confirmation when a fresh snapshot shows tabs that cannot continue", async () => {
  snapshot.continuation = "unverified";
  snapshot.accounts[1] = { ...snapshot.accounts[1], tabsInUse: 0 };
  await render();
  await click("Sign out and remove", await menu("work"));
  snapshot = { ...snapshot, accounts: [snapshot.accounts[0], { ...snapshot.accounts[1], tabsInUse: 1 }] };
  await render();
  expect(Array.from(row("work").querySelectorAll("button")).some(node => node.textContent === "Remove")).toBe(false);
  expect(document.activeElement).toBe(button("Actions for other@example.test"));
  expect(onAction).not.toHaveBeenCalled();
});

it("retains the row on a race refusal and dismisses only the exact accepted notice with focus restoration", async () => {
  snapshot.continuation = "unverified";
  snapshot.accounts[1] = { ...snapshot.accounts[1], tabsInUse: 0 };
  await render();
  await click("Sign out and remove", await menu("work"));
  await click("Remove", row("work"));
  snapshot = { ...snapshot, accounts: [snapshot.accounts[0], { ...snapshot.accounts[1], tabsInUse: 2, removalNotice: { id: "notice-one", kind: "inUse" } }] };
  await render();
  expect(host.querySelectorAll("[data-account-id]")).toHaveLength(2);
  expect(row("work").querySelector('[role="status"]')?.textContent).toContain("Close this account's tabs before removing it.");
  expect(Array.from(row("work").querySelectorAll("button")).some(node => node.textContent === "Remove")).toBe(false);
  await click("Got it", row("work"));
  expect(onAction).toHaveBeenLastCalledWith({ action: "dismissRemovalNotice", accountId: "work", noticeId: "notice-one" });
  snapshot = { ...snapshot, accounts: [snapshot.accounts[0], { ...snapshot.accounts[1], removalNotice: undefined }] };
  await render();
  expect(row("work").querySelector('[role="status"]')).toBeNull();
  expect(document.activeElement).toBe(button("Actions for other@example.test"));
});

it("keeps a newer removal notice when an earlier dismissal resolves", async () => {
  snapshot.accounts[1] = { ...snapshot.accounts[1], removalNotice: { id: "old-notice", kind: "inUse" } };
  let resolve!: () => void;
  onAction.mockImplementationOnce(() => new Promise<void>(done => { resolve = done; }));
  await render();
  await click("Got it", row("work"));
  expect(onAction).toHaveBeenLastCalledWith({ action: "dismissRemovalNotice", accountId: "work", noticeId: "old-notice" });
  snapshot = { ...snapshot, accounts: [snapshot.accounts[0], { ...snapshot.accounts[1], removalNotice: { id: "new-notice", kind: "inUse" } }] };
  await render();
  await act(async () => resolve());
  expect(button("Got it", row("work")).disabled).toBe(false);
  await click("Got it", row("work"));
  expect(onAction).toHaveBeenLastCalledWith({ action: "dismissRemovalNotice", accountId: "work", noticeId: "new-notice" });
});

it("uses account-add progress hints instead of promising a hidden context will continue a conversation", async () => {
  const context = { tabId: "hidden-account-login", state: { state: "loginPending" as const, epoch: 20,
    login: { status: "pending" as const, url: "https://example.test/login" } } };
  snapshot.adding = { context, status: "signingIn" };
  await render();
  expect(login.render).toHaveBeenLastCalledWith(expect.objectContaining({ hint: "Finish signing in to add this account." }));
  snapshot = { ...snapshot, adding: { context: { ...context, state: { ...context.state, notice: "opaque account-check progress" } }, status: "signingIn" } };
  await render();
  expect(login.render).toHaveBeenLastCalledWith(expect.objectContaining({ hint: "opaque account-check progress" }));
});

it("renders a restored removal failure with collapsed details, without moving focus or retrying automatically", async () => {
  await render();
  row("default").focus();
  snapshot = { ...snapshot, accounts: [snapshot.accounts[0], { ...snapshot.accounts[1], status: "removing", tabsInUse: 0, removalFailureReason: "opaque deletion failure" }] };
  await render();
  const notice = row("work").querySelector<HTMLElement>('[role="status"]')!;
  expect(row("work").getAttribute("aria-label")).toBe("other@example.test, couldn't finish removing");
  expect(notice.querySelector("p")?.textContent).toBe("Couldn't finish removing other@example.test.");
  expect(notice.querySelector("details")?.open).toBe(false);
  expect(notice.querySelector("details")?.textContent).toContain("opaque deletion failure");
  expect(notice.querySelector("p")?.textContent).not.toContain("opaque deletion failure");
  expect(button("Try again", notice)).toBeDefined();
  expect(notice.querySelector('button[aria-label="Dismiss notice"]')).toBeNull();
  expect(document.activeElement).toBe(row("default"));
  snapshot = { ...snapshot, revision: "unrelated-refresh" };
  await render();
  expect(row("work").querySelector('[role="status"]')).toBe(notice);
  expect(document.activeElement).toBe(row("default"));
  expect(onAction).not.toHaveBeenCalled();
});

it("keeps failed removal stationary with only an explicit menu retry, then updates failure details after the attempt", async () => {
  snapshot.accounts[1] = { ...snapshot.accounts[1], status: "removing", tabsInUse: 0, removalFailureReason: "first failure" };
  let resolve!: () => void;
  onAction.mockImplementationOnce(() => new Promise<void>(done => { resolve = done; }));
  await render();
  const actions = await menu("work");
  for (const label of ["Move to top", "Move up", "Move down", "Rename", "Sign out and remove"]) {
    const item = button(label, actions);
    expect(item.disabled).toBe(false);
    expect(item.getAttribute("aria-disabled")).toBe("true");
    expect(document.getElementById(item.getAttribute("aria-describedby")!)?.textContent).toBe("Finishing removal first.");
    await act(async () => item.click());
  }
  await act(async () => row("work").dispatchEvent(new KeyboardEvent("keydown", { code: "ArrowUp", altKey: true, bubbles: true })));
  expect(onAction).not.toHaveBeenCalled();
  await click("Try again", actions);
  expect(onAction).toHaveBeenCalledExactlyOnceWith({ action: "remove", accountId: "work" });
  expect(row("work").querySelector('[role="status"][aria-label="Removing"]')).not.toBeNull();
  expect(row("work").querySelector("details")).toBeNull();
  snapshot = { ...snapshot, accounts: [snapshot.accounts[0], { ...snapshot.accounts[1], removalFailureReason: "second failure" }] };
  await render();
  await act(async () => resolve());
  expect(row("work").querySelector("details")?.open).toBe(false);
  expect(row("work").querySelector("details")?.textContent).toContain("second failure");
  expect(row("work").querySelector("details")?.textContent).not.toContain("first failure");
  expect(onAction).toHaveBeenCalledTimes(1);
});

it("does not replace a persisted failure with Removing while an unrelated account action runs", async () => {
  snapshot.accounts[1] = { ...snapshot.accounts[1], status: "removing", removalFailureReason: "retained failure" };
  let resolve!: () => void;
  onAction.mockImplementationOnce(() => new Promise<void>(done => { resolve = done; }));
  await render();
  await click("Add account");
  expect(row("work").querySelector("details")?.textContent).toContain("retained failure");
  expect(row("work").querySelector('[role="status"][aria-label="Removing"]')).toBeNull();
  await act(async () => resolve());
});

it("withdraws an open editor when a removal failure arrives from another client", async () => {
  await render();
  await click("Rename", await menu("work"));
  expect(host.querySelector("input")).not.toBeNull();
  snapshot = { ...snapshot, accounts: [snapshot.accounts[0], { ...snapshot.accounts[1], status: "removing", removalFailureReason: "external cleanup failed" }] };
  await render();
  expect(host.querySelector("input")).toBeNull();
  expect(row("work").querySelector("details")?.textContent).toContain("external cleanup failed");
  expect(document.activeElement).toBe(button("Actions for other@example.test"));
  expect(onAction).not.toHaveBeenCalled();
});

it("shows the active cleanup line after the parent clears the persisted reason", async () => {
  snapshot.accounts[1] = { ...snapshot.accounts[1], status: "removing", tabsInUse: 0, tabsToFinish: 0, removalFailureReason: "previous failure" };
  await render();
  await click("Try again", row("work"));
  snapshot = { ...snapshot, accounts: [snapshot.accounts[0], { ...snapshot.accounts[1], removalFailureReason: undefined }] };
  await render();
  expect(row("work").querySelector('[role="status"][aria-label="Removing"]')).not.toBeNull();
  expect(row("work").querySelector("details")).toBeNull();
  expect(onAction).toHaveBeenCalledExactlyOnceWith({ action: "remove", accountId: "work" });
});

it.each([true, false])("announces accepted removal once and focuses its surviving next row (present: %s) after the request settles", async hasNext => {
  snapshot.accounts[1] = { ...snapshot.accounts[1], status: "removing", tabsInUse: 0, removalFailureReason: "cleanup failed" };
  if (hasNext) snapshot.accounts.push({ id: "next", identity: "next@example.test", status: "ready", tabsInUse: 0 });
  let resolve!: () => void;
  onAction.mockImplementationOnce(() => new Promise<void>(done => { resolve = done; }));
  await render();
  await click("Try again", row("work"));
  snapshot = { ...snapshot, accounts: snapshot.accounts.filter(account => account.id !== "work"),
    feedback: { id: "removed-once", kind: "removed", accountId: "work", identity: "other@example.test", position: 1 } };
  await render();
  expect(button("Add account").disabled).toBe(true);
  await act(async () => resolve());
  expect(host.querySelector('[aria-atomic="true"]')?.textContent).toBe("Removed other@example.test");
  expect(document.activeElement).toBe(hasNext ? row("next") : button("Add account"));
  row("default").focus();
  snapshot = { ...snapshot, revision: "same-accepted-feedback" };
  await render();
  expect(document.activeElement).toBe(row("default"));
});
