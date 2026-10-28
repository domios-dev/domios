import { bridge, account, neighbor, mounted, button, hasButton, emit, runtime, render, renderSection, concierge, pendingClient, act, createRoot, expect, it, vi, SidebarNeighborsSection, takeNeighborFocus, missingConciergeLine, useNeighborGatewayIds } from "./neighborsView.testdata";
import type { NeighborClient, NeighborList, ReferralClaim, NeighborRuntimeClient } from "./neighborsView.testdata";


it("keeps the sidebar Neighbors section away until there is a Neighbor", async () => {
  await renderSection([]);
  expect(document.querySelector('[data-testid="neighbors-section-toggle"]')).toBeNull();
});

it("updates the sidebar immediately from an own-account push and removes its subscription on unmount", async () => {
  await renderSection([]);
  const list = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  await act(async () => { emit("neighbors-changed", { uid: "owner", list }); });
  expect(hasButton("Open Ada")).toBe(true);
  await act(async () => mounted!.root.unmount());
  expect(bridge.handlers["neighbors-changed"]).toBeUndefined();
  mounted!.host.remove();
  mounted = null;
});

it("nests each concierge under its Neighbor; the Neighbor opens your conversation and the concierge its chat", async () => {
  await renderSection([neighbor, { ...neighbor, uid: "peer-2", displayName: "Lin", friendship_id: "pair-2" }], { pair: "tab-gateway" });
  const focused = vi.fn();
  const panels = vi.fn();
  window.addEventListener("codeterm:focus-tab", focused);
  window.addEventListener("codeterm:open-panel", panels);
  try {
    expect(hasButton("Open Lin's concierge")).toBe(false);
    await act(async () => button("Open Ada's concierge").click());
    expect((focused.mock.calls[0][0] as CustomEvent).detail).toEqual({ tabId: "tab-gateway", workspaceId: "ws-work" });
    await act(async () => button("Open Lin").click());
    expect((panels.mock.calls[0][0] as CustomEvent).detail).toEqual({ panel: "neighbor:pair-2" });
  } finally {
    window.removeEventListener("codeterm:focus-tab", focused);
    window.removeEventListener("codeterm:open-panel", panels);
  }
});

it("offers the concierge actions: change agent, view permissions, edit with General and pause", async () => {
  await renderSection([neighbor], { pair: "tab-gateway" });
  await act(async () => { button("Actions for Ada's concierge").dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 })); });
  const items = Array.from(document.querySelectorAll('[role="menuitem"]')).map(item => item.textContent);
  expect(items).toEqual(["Change agent", "View permissions", "Edit with General", "Disable Ada"]);
  const viewPermissions = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(item => item.textContent === "View permissions")!;
  await act(async () => viewPermissions.click());
  expect(takeNeighborFocus()).toEqual({ id: "pair", section: "permissions" });
});

it("nests a concierge bound from any workspace under its friend and marks it hidden from workspace lists", async () => {
  const seen: Array<ReadonlySet<string>> = [];
  function Probe() { seen.push(useNeighborGatewayIds()); return null; }
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, incoming: [], outgoing: [],
    friends: [{ ...neighbor, gateway_tab: "tab-gateway" }] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<><SidebarNeighborsSection touchMode={false} client={client} runtimeClient={runtime()} /><Probe /></>));
  expect(hasButton("Open Ada's concierge")).toBe(true);
  expect(seen[seen.length - 1]?.has("tab-gateway")).toBe(true);
});

it("draws last known friends at once with a quiet refresh indicator, and stays away when there were none", async () => {
  localStorage.setItem("domios.neighbors.last-known:owner", JSON.stringify({ access: "paid", friends: [{ friendship_id: "pair", uid: "peer", displayName: "Ada", photoURL: null, gateway_tab: "tab-gateway" }] }));
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<SidebarNeighborsSection touchMode={false} client={pendingClient()} runtimeClient={runtime()} />));
  expect(hasButton("Open Ada")).toBe(true);
  expect(hasButton("Open Ada's concierge")).toBe(true);
  expect(document.querySelector('[aria-label="Refreshing Neighbors"]')).not.toBeNull();
  localStorage.setItem("domios.neighbors.last-known:owner", JSON.stringify({ access: "paid", friends: [] }));
  await act(async () => root.render(<SidebarNeighborsSection key="empty" touchMode={false} client={pendingClient()} runtimeClient={runtime()} />));
  expect(document.querySelector('[data-testid="neighbors-section-toggle"]')).toBeNull();
});

it("highlights an open concierge chat like an open tab, without the friend row", async () => {
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [{ ...neighbor, gateway_tab: "tab-gateway" }], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  const runtimeClient = runtime();
  await act(async () => root.render(<SidebarNeighborsSection touchMode={false} client={client} runtimeClient={runtimeClient} activeTabId="tab-gateway" />));
  expect(button("Open Ada's concierge").getAttribute("aria-current")).toBe("page");
  expect(button("Open Ada's concierge").closest("[data-tab-active]")?.querySelector(".active-tab-pill")).not.toBeNull();
  expect(button("Open Ada").getAttribute("aria-current")).toBeNull();
  await act(async () => root.render(<SidebarNeighborsSection touchMode={false} client={client} runtimeClient={runtimeClient} activeTabId="tab-general" />));
  expect(button("Open Ada's concierge").getAttribute("aria-current")).toBeNull();
});

it("gives the friend and concierge rows the sidebar's own hover kebab", async () => {
  await renderSection([{ ...neighbor, gateway_tab: "tab-gateway" }]);
  expect(button("Actions for Ada").closest(".sidebar-tab-kebab")).not.toBeNull();
  expect(button("Actions for Ada's concierge").closest(".sidebar-tab-kebab")).not.toBeNull();
});

it("remembers only names, avatars and the concierge binding, and forgets them on sign-out", async () => {
  await renderSection([{ ...neighbor, gateway_tab: "tab-gateway", my_daily_limit: 5 }]);
  const stored = JSON.parse(localStorage.getItem("domios.neighbors.last-known:owner")!);
  expect(stored).toEqual({ access: "paid", can_invite: true, incoming: [], outgoing: [],
    friends: [{ friendship_id: "pair", uid: "peer", displayName: "Ada", photoURL: null, gateway_tab: "tab-gateway" }] });
  account.model = { kind: "signed_out" };
  await act(async () => mounted!.root.render(<SidebarNeighborsSection touchMode={false} runtimeClient={runtime()} />));
  expect(localStorage.getItem("domios.neighbors.last-known:owner")).toBeNull();
});

it("never claims a missing concierge when one is bound, and omits a zero count", async () => {
  expect(missingConciergeLine({ gateway_tab: null, concierge_missing: true, pending_count: 0, receiving_here: true, receiver_machine_label: null, state: "concierge_missing" }))
    .toBe("No concierge on this machine");
  const runtimeClient = runtime({ pair: "tab-gateway" });
  vi.mocked(runtimeClient.status).mockResolvedValue({ enrolled: true, bindings: { pair: "tab-gateway" }, receiving_here: true, receiving_enabled: true, receiver_machine_label: null, revision: 1,
    concierges: { pair: { gateway_tab: null, concierge_missing: true, pending_count: 2, receiving_here: true, receiver_machine_label: null, state: "concierge_missing" } } } as unknown as Awaited<ReturnType<NeighborRuntimeClient["status"]>>);
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<SidebarNeighborsSection touchMode={false} client={client} runtimeClient={runtimeClient} />));
  expect(hasButton("Open Ada's concierge")).toBe(true);
  expect(document.body.textContent).not.toContain("no concierge here");
});

it("disables a friend from their sidebar row menu and shows the row as disabled", async () => {
  const conciergeClient = concierge([]);
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, incoming: [], outgoing: [], friends: [neighbor] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<SidebarNeighborsSection touchMode={false} client={client} runtimeClient={runtime()} conciergeClient={conciergeClient} />));
  await act(async () => { button("Actions for Ada").dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 })); });
  await act(async () => Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(item => item.textContent === "Disable Ada")!.click());
  expect(conciergeClient.setPaused).toHaveBeenCalledExactlyOnceWith("pair", true);
  expect(button("Open Ada").textContent).toContain("disabled");
});
