import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ProviderDescriptor, Tab, Workspace } from "../../lib/types";
import type { ProviderAccountsSnapshot } from "../../hooks/providerAccountsTypes";

const fixture = vi.hoisted(() => ({
  descriptor: {} as ProviderDescriptor,
  tabAccounts: { snapshot: null as ProviderAccountsSnapshot | null, currentAccountId: "default", currentIdentity: "usual@example.test" as string | null,
    switching: false, canVerify: false, turnActive: true, blockedReason: null as string | null, error: null as string | null, choose: vi.fn() },
  workspaceAccounts: { providers: [] as { providerId: string; displayName: string; snapshot: ProviderAccountsSnapshot }[], loading: false, error: null, choose: vi.fn() },
  tabHook: vi.fn(), workspaceHook: vi.fn(), published: vi.fn(),
}));
vi.mock("../../hooks/useAccountSelectors", () => ({
  useTabAccounts: (tab: Tab) => { fixture.tabHook(tab); return fixture.tabAccounts; },
  useWorkspaceAccounts: (id: string) => { fixture.workspaceHook(id); return fixture.workspaceAccounts; },
}));
vi.mock("../../lib/useAgentDescriptor", () => ({ useAgentDescriptor: () => fixture.descriptor }));
vi.mock("../../hooks/useAgentProviders", () => ({ useAgentProviders: () => ({ providers: [fixture.descriptor], loading: false, error: null }) }));
vi.mock("../../hooks/useShellProfiles", () => ({ useShellProfiles: () => ({ profiles: [], refresh: vi.fn(), loading: false, error: null }) }));
vi.mock("../../hooks/useConfig", () => ({ useConfig: () => ({ config: { agents: {} } }) }));
vi.mock("../../lib/notices", async original => ({ ...await original<typeof import("../../lib/notices")>(), publishNotice: fixture.published }));
import { WorkspaceItem } from "../Sidebar/WorkspaceItem";
import { workspaceAccountMenu } from "../Sidebar/workspaceAccountMenu";
import { SpawnAccountPill } from "./SpawnAccountPill";
import { TabMenu } from "./TabMenu";
import { TabActionsMenu } from "./TabActionsMenu";
import { TabStatusLineModelPicker } from "./StatusLineModelPicker";
import { tabAccountSubmenu } from "./tabMenuModel";

const ownTab = { id: "own-tab", agent_type: "opaque-harness", pty_id: "own-pty", title: "Agent", agent_session_model: "model", restorable_launch: { account_id: "default" } } as Tab;
const workspace = { id: "own-workspace", name: "Project", tabs: [], active_tab_id: null, cwd: "/project" } as Workspace;
function snapshot(): ProviderAccountsSnapshot {
  return { providerId: "opaque-harness", revision: "rev", continuation: "proven", automaticSwitch: true, workspaceOverrides: [], accounts: [
    { id: "default", identity: "usual@example.test", status: "ready", tabsInUse: 1 },
    { id: "work", name: "Work", identity: "work@example.test", status: "exhausted", resetsAtMs: 2_000_000_000_000, tabsInUse: 0 },
    { id: "out", identity: "out@example.test", status: "signedOut", tabsInUse: 0 },
    { id: "removing", identity: "removing@example.test", status: "removing", tabsInUse: 1 },
  ] };
}
let host: HTMLDivElement;
let root: Root;
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
beforeEach(() => {
  fixture.descriptor = { id: "opaque-harness", display_name: "Fixture", accounts: {}, model_switch: "next_turn",
    available_models: [{ id: "model", display_name: "Model" }], reasoning_efforts: [] } as unknown as ProviderDescriptor;
  Object.assign(fixture.tabAccounts, { snapshot: snapshot(), currentAccountId: "default", currentIdentity: "usual@example.test", switching: false, canVerify: false, turnActive: true, blockedReason: null, error: null });
  fixture.tabAccounts.choose.mockReset().mockResolvedValue(undefined);
  fixture.workspaceAccounts.providers = [{ providerId: "opaque-harness", displayName: "Fixture", snapshot: snapshot() }];
  fixture.workspaceAccounts.choose.mockReset().mockResolvedValue(undefined);
  fixture.tabHook.mockClear(); fixture.workspaceHook.mockClear(); fixture.published.mockClear();
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(async () => { await act(async () => root.unmount()); host.remove(); });
async function render(element: ReactNode) { await act(async () => root.render(element)); }
function button(name: string, role?: string) {
  const found = Array.from(document.querySelectorAll<HTMLButtonElement>(role ? `button[role="${role}"]` : "button"))
    .find(node => node.getAttribute("aria-label")?.startsWith(name) || node.textContent?.startsWith(name));
  if (!found) throw new Error(`Missing ${role ?? "button"}: ${name}`);
  return found;
}
async function open(name: string) { await act(async () => button(name).dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, cancelable: true }))); }
async function click(name: string, role?: string) { await act(async () => button(name, role).click()); }
function workspaceView() {
  return <WorkspaceItem workspace={workspace} isActive isExpanded={false} onClick={vi.fn()} onToggleExpand={vi.fn()} onClose={vi.fn()}
    onRename={vi.fn()} onNewTerminal={vi.fn()} onNewSession={vi.fn()} onNewAgent={vi.fn()} canClose status={null} />;
}

it("wires the workspace submenu to exact provider/account choices, including First available", async () => {
  fixture.workspaceAccounts.providers[0].snapshot.workspaceOverrides = [{ workspaceId: workspace.id, workspaceName: workspace.name, accountId: "work" }];
  await render(workspaceView());
  expect(fixture.workspaceHook).toHaveBeenCalledWith(workspace.id);
  await open("Actions for Project"); await click("Accounts", "menuitem");
  expect(button("Work", "menuitemradio").getAttribute("aria-checked")).toBe("true");
  const out = button("out@example.test", "menuitemradio");
  expect(out.disabled).toBe(false);
  expect(document.getElementById(out.getAttribute("aria-describedby")!)?.textContent).toBe("Signed out");
  expect(document.querySelector('[role="menu"]')?.textContent).not.toContain("removing@example.test");
  await click("out@example.test", "menuitemradio");
  expect(fixture.workspaceAccounts.choose).toHaveBeenLastCalledWith("opaque-harness", "out");
  await open("Actions for Project"); await click("Accounts", "menuitem"); await click("First available", "menuitemradio");
  expect(fixture.workspaceAccounts.choose).toHaveBeenLastCalledWith("opaque-harness", null);
});

it("omits workspace selectors for one account and incapable harnesses", async () => {
  const providers = fixture.workspaceAccounts.providers;
  expect(workspaceAccountMenu(workspace.id, [{ ...providers[0], snapshot: { ...providers[0].snapshot, accounts: providers[0].snapshot.accounts.slice(0, 1) } }], vi.fn())).toBeNull();
  fixture.descriptor = { ...fixture.descriptor, accounts: undefined } as ProviderDescriptor;
  await render(workspaceView()); await open("Actions for Project");
  expect(Array.from(document.querySelectorAll('[role="menuitem"]')).some(node => node.textContent === "Accounts")).toBe(false);
});

it("keeps workspace exhaustion selectable with its own described state", async () => {
  await render(workspaceView()); await open("Actions for Project"); await click("Accounts", "menuitem");
  const work = button("Work", "menuitemradio");
  expect(work.disabled).toBe(false);
  expect(document.getElementById(work.getAttribute("aria-describedby")!)?.textContent).toContain("Limit reached, resets");
  await click("Work", "menuitemradio");
  expect(fixture.workspaceAccounts.choose).toHaveBeenLastCalledWith("opaque-harness", "work");
});

it("uses the shared spawn radio menu, forwards status descriptions, and returns focus to the value-bearing pill", async () => {
  const select = vi.fn();
  await render(<SpawnAccountPill snapshot={snapshot()} providerName="Fixture" selectedAccountId="work" onSelect={select} />);
  const pill = button("Account: Work");
  expect(pill.getAttribute("aria-haspopup")).toBe("menu");
  await open("Account: Work");
  expect(button("Work", "menuitemradio").getAttribute("aria-checked")).toBe("true");
  await click("out@example.test", "menuitemradio");
  expect(select).toHaveBeenCalledExactlyOnceWith("out");
  expect(document.activeElement).toBe(pill);
  expect(host.textContent).toBe("Work");
});

it("hides the spawn pill when fewer than two non-removing accounts remain", async () => {
  const value = snapshot(); value.accounts = [value.accounts[0], value.accounts[3]];
  await render(<SpawnAccountPill snapshot={value} providerName="Fixture" selectedAccountId="default" onSelect={vi.fn()} />);
  expect(host.querySelector("button")).toBeNull();
});

it.each(["sidebar", "header"])("uses the same proven tab-account model and exact bound tab on the %s surface", async surface => {
  const props = { entries: [], onAction: vi.fn(), tab: ownTab };
  await render(surface === "sidebar" ? <TabMenu {...props} trigger={<button>Tab actions</button>} /> : <TabActionsMenu {...props} busy={false} />);
  expect(fixture.tabHook).toHaveBeenCalledWith(ownTab);
  await open("Tab actions"); await click("Account", "menuitem");
  expect(button("usual@example.test", "menuitemradio").getAttribute("aria-checked")).toBe("true");
  expect(button("out@example.test", "menuitemradio").getAttribute("aria-disabled")).toBe("true");
  await click("Work", "menuitemradio");
  expect(fixture.tabAccounts.choose).toHaveBeenCalledExactlyOnceWith("work");
  expect(props.onAction).not.toHaveBeenCalled();
});

it.each(["unverified", "unsupported"] as const)("keeps alternative live-tab accounts focusable and inert when continuation is %s", async continuation => {
  fixture.tabAccounts.snapshot!.continuation = continuation;
  fixture.tabAccounts.blockedReason = "opaque continuation reason";
  await render(<TabMenu tab={ownTab} entries={[]} onAction={vi.fn()} trigger={<button>Tab actions</button>} />);
  await open("Tab actions"); await click("Account", "menuitem");
  const work = button("Work", "menuitemradio"); work.focus();
  expect(document.activeElement).toBe(work);
  expect(work.disabled).toBe(false);
  expect(work.getAttribute("aria-disabled")).toBe("true");
  expect(document.getElementById(work.getAttribute("aria-describedby")!)?.textContent).toBe("opaque continuation reason");
  await click("Work", "menuitemradio");
  expect(fixture.tabAccounts.choose).not.toHaveBeenCalled();
});

it("disables duplicate/current tab choices and excludes removing accounts", () => {
  const menu = tabAccountSubmenu({ ...fixture.tabAccounts, switching: true }, "Fixture")!;
  expect(menu.entries).toHaveLength(3);
  expect(menu.entries.every(entry => entry.kind === "item" && entry.disabled && entry.action === null)).toBe(true);
});

it("surfaces a tab switch refusal through the existing toast path", async () => {
  fixture.tabAccounts.choose.mockRejectedValueOnce(new Error("opaque refused request"));
  await render(<TabMenu tab={ownTab} entries={[]} onAction={vi.fn()} trigger={<button>Tab actions</button>} />);
  await open("Tab actions"); await click("Account", "menuitem"); await click("Work", "menuitemradio");
  expect(fixture.published).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ kind: "toast", tabId: ownTab.id }));
});

it.each([{ locked: true }, { role_profile: "curator" as const }])("preserves existing tab authority gates for $locked $role_profile", async gate => {
  await render(<TabMenu tab={{ ...ownTab, ...gate }} entries={[]} onAction={vi.fn()} trigger={<button>Tab actions</button>} />);
  await open("Tab actions");
  expect(Array.from(document.querySelectorAll('[role="menuitem"]')).some(node => node.textContent === "Account")).toBe(false);
});

it("adds the current account to the model tooltip only when a capable harness has multiple accounts", async () => {
  await render(<TabStatusLineModelPicker tab={ownTab} label="Fixture · Model" />);
  const pill = host.querySelector<HTMLElement>('[title*="usual@example.test"]');
  expect(pill).not.toBeNull();
  expect(pill!.getAttribute("title")?.split("\n")[1]).toBe("usual@example.test");
  expect(host.textContent).not.toContain("usual@example.test");
  fixture.tabAccounts.snapshot = { ...snapshot(), accounts: snapshot().accounts.slice(0, 1) };
  await render(<TabStatusLineModelPicker tab={ownTab} label="Fixture · Model" />);
  expect(host.querySelector('[title*="usual@example.test"]')).toBeNull();
});

it("does not mount account subscriptions on tab surfaces for an incapable harness", async () => {
  fixture.descriptor = { ...fixture.descriptor, accounts: undefined } as ProviderDescriptor;
  await render(<><TabStatusLineModelPicker tab={ownTab} label="Fixture · Model" />
    <TabMenu tab={ownTab} entries={[]} onAction={vi.fn()} trigger={<button>Tab actions</button>} /></>);
  expect(fixture.tabHook).not.toHaveBeenCalled();
});

function ownerTrial() {
  fixture.tabAccounts.snapshot!.continuation = "unverified";
  fixture.tabAccounts.snapshot!.accounts[1].status = "ready";
  Object.assign(fixture.tabAccounts, { canVerify: true, turnActive: false, blockedReason: "Moving this conversation to another account isn't confirmed yet." });
  return <TabMenu tab={ownTab} entries={[]} onAction={vi.fn()} trigger={<button>Tab actions</button>} />;
}

it("uses the explicit idle owner's radio choice as one trial through the parent's choose contract", async () => {
  await render(ownerTrial());
  await open("Tab actions"); await click("Account", "menuitem");
  const active = document.querySelector<HTMLElement>('[role="menu"]:not([hidden])')!;
  const explanation = document.getElementById(active.getAttribute("aria-describedby")!);
  expect(explanation?.textContent).toBe("Choosing an account tries this conversation there. Its history stays the same.");
  expect(active.contains(explanation)).toBe(true);
  const current = button("usual@example.test", "menuitemradio");
  expect(current.getAttribute("aria-checked")).toBe("true");
  expect(current.disabled).toBe(false);
  expect(document.getElementById(current.getAttribute("aria-describedby")!)?.textContent).toBe("Wait for this turn to finish");
  const work = button("Work", "menuitemradio");
  expect(work.disabled).toBe(false);
  expect(work.getAttribute("aria-disabled")).toBeNull();
  await click("Work", "menuitemradio");
  expect(fixture.tabAccounts.choose).toHaveBeenCalledExactlyOnceWith("work");
});

it("keeps a nonowner's ready unverified target inert without the trial explanation", async () => {
  const view = ownerTrial(); fixture.tabAccounts.canVerify = false;
  await render(view); await open("Tab actions"); await click("Account", "menuitem");
  const active = document.querySelector<HTMLElement>('[role="menu"]:not([hidden])')!;
  expect(active.getAttribute("aria-describedby")).toBeNull();
  const work = button("Work", "menuitemradio");
  expect(work.getAttribute("aria-disabled")).toBe("true");
  await click("Work", "menuitemradio");
  expect(fixture.tabAccounts.choose).not.toHaveBeenCalled();
});

it.each(["turn", "request"])("blocks another owner trial while a %s is active", async busy => {
  const view = ownerTrial();
  fixture.tabAccounts.turnActive = busy === "turn";
  fixture.tabAccounts.switching = busy === "request";
  await render(view); await open("Tab actions"); await click("Account", "menuitem");
  const work = button("Work", "menuitemradio");
  expect(work.disabled || work.getAttribute("aria-disabled") === "true").toBe(true);
  if (busy === "turn") expect(document.getElementById(work.getAttribute("aria-describedby")!)?.textContent).toBe("Wait for this turn to finish");
  await click("Work", "menuitemradio");
  expect(fixture.tabAccounts.choose).not.toHaveBeenCalled();
});

it.each([
  { status: "signedOut", reason: "Sign in first" },
  { status: "exhausted", reason: "Limit reached, resets" },
  { status: "unknown", reason: "This account isn't ready." },
] as const)("keeps an owner's $status target inert with an accessible reason", async ({ status, reason }) => {
  const view = ownerTrial(); fixture.tabAccounts.snapshot!.accounts[1].status = status;
  await render(view); await open("Tab actions"); await click("Account", "menuitem");
  const work = button("Work", "menuitemradio"); work.focus();
  expect(document.activeElement).toBe(work);
  expect(work.getAttribute("aria-disabled")).toBe("true");
  expect(document.getElementById(work.getAttribute("aria-describedby")!)?.textContent).toContain(reason);
  await click("Work", "menuitemradio");
  expect(fixture.tabAccounts.choose).not.toHaveBeenCalled();
});

it("does not let stale owner-trial authority enable an unsupported harness", async () => {
  const view = ownerTrial(); fixture.tabAccounts.snapshot!.continuation = "unsupported";
  fixture.tabAccounts.blockedReason = "Fixture can't move this conversation";
  await render(view); await open("Tab actions"); await click("Account", "menuitem");
  const active = document.querySelector<HTMLElement>('[role="menu"]:not([hidden])')!;
  expect(active.getAttribute("aria-describedby")).toBeNull();
  await click("Work", "menuitemradio");
  expect(fixture.tabAccounts.choose).not.toHaveBeenCalled();
});

it("removes the trial explanation when the parent projects proven continuation and retains safe-boundary choices", async () => {
  await render(ownerTrial()); await open("Tab actions"); await click("Account", "menuitem");
  fixture.tabAccounts.snapshot!.continuation = "proven";
  fixture.tabAccounts.blockedReason = null; fixture.tabAccounts.turnActive = true;
  await render(<TabMenu tab={ownTab} entries={[]} onAction={vi.fn()} trigger={<button>Tab actions</button>} />);
  const active = document.querySelector<HTMLElement>('[role="menu"]:not([hidden])')!;
  expect(active.getAttribute("aria-describedby")).toBeNull();
  expect(active.textContent).not.toContain("Choosing an account tries");
  await click("Work", "menuitemradio");
  expect(fixture.tabAccounts.choose).toHaveBeenCalledExactlyOnceWith("work");
});
