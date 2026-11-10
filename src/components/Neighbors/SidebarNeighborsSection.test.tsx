import { afterEach, describe, it, expect, vi } from "vitest";
import { act } from "react";
import { takeNeighborFocus } from "./neighborFocus";
import { useNeighborGatewayIds } from "./neighborGateways";
import type { NeighborList } from "./neighborModel";
import type { NeighborClient } from "./neighborClient";
import { createRoot } from "react-dom/client";
import { SidebarNeighborsSection } from "./SidebarNeighborsSection";
import { resetNeighborReads } from "./neighborReads";
import { missingConciergeLine } from "./useConciergeStatus";
import type { NeighborRuntimeClient } from "./neighborRuntimeClient";
import { createNeighborsViewFixture } from "./neighborsView.testdata";

const notices = vi.hoisted(() => ({ publishNotice: vi.fn() }));
const transport = vi.hoisted(() => ({ localFetch: vi.fn(), webJson: vi.fn() }));
const bridge = vi.hoisted(() => ({ handlers: {} as Record<string, Set<(event: { payload: unknown }) => void>>, invoke: vi.fn(async () => undefined) }));
const account = vi.hoisted(() => ({ model: { kind: "signed_in", identity: { uid: "owner", email: "me@example.test", display_name: "Me", photo_url: null } } as Record<string, unknown>, signIn: vi.fn() }));
const mux = vi.hoisted(() => ({
  workspaces: [{ id: "ws-general", name: "General", tabs: ["tab-general"], protected: true }, { id: "ws-work", name: "Work", tabs: ["tab-gateway"] }],
  tabs: { "tab-general": { id: "tab-general", title: "Main", protected: true }, "tab-gateway": { id: "tab-gateway", title: "Concierge" } },
  tabStatuses: {},
}));
vi.mock("../../lib/webApi", async actual => ({ ...await actual<object>(), ...transport }));
vi.mock("../../lib/notices", () => notices);
vi.mock("../../lib/bridge", () => ({ invoke: bridge.invoke, listen: vi.fn(async (event: string, handler: (event: { payload: unknown }) => void) => {
  const handlers = bridge.handlers[event] ??= new Set();
  handlers.add(handler);
  return () => { handlers.delete(handler); if (handlers.size === 0) delete bridge.handlers[event]; };
}) }));
vi.mock("../Settings/useAccount", () => ({ useAccount: () => ({ model: account.model, signIn: account.signIn, signOut: vi.fn() }) }));
vi.mock("../../store/multiplexer/hooks", () => ({ useMuxSelector: (select: (state: typeof mux) => unknown) => select(mux) }));

const { neighbor, mounted, button, hasButton, emit, runtime, renderSection, concierge, pendingClient } = createNeighborsViewFixture({ notices, transport, bridge, account, mux });



async function expandSection() {
  const toggle = document.querySelector<HTMLButtonElement>('section[aria-label="Neighbors"] button[aria-expanded="false"]');
  if (toggle) await act(async () => toggle.click());
}

it("groups the selected remote peer's concierge without reading or managing the origin account", async () => {
  const { setRelayTarget } = await import("../../lib/webApi");
  setRelayTarget("blackstar", "Blackstar");
  transport.webJson.mockResolvedValue({ pair: { tab_id: "tab-gateway", friend_display_name: "Remote Friend", friend_photo_url: "https://example.test/friend.png", paused: false, access: "paid" } });
  const localClient: NeighborClient = { list: vi.fn(), apply: vi.fn() };
  const localRuntime = runtime({ pair: "tab-general" });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  const seen: Array<ReadonlySet<string>> = [];
  function Probe() { seen.push(useNeighborGatewayIds()); return null; }
  await act(async () => root.render(<><SidebarNeighborsSection touchMode={false} activeChat="pair" client={localClient} runtimeClient={localRuntime} /><Probe /></>));
  await expandSection();
  const friend = button("Open Remote Friend");
  expect(friend.getAttribute("aria-current")).toBe("page");
  expect(friend.querySelector("img")?.getAttribute("src")).toBe("https://example.test/friend.png");
  expect(friend.textContent).toContain("connected");
  const opened = vi.fn();
  window.addEventListener("codeterm:open-panel", opened);
  try {
    await act(async () => friend.click());
    expect((opened.mock.calls[0][0] as CustomEvent).detail).toEqual({ panel: "neighbor:pair" });
  } finally {
    window.removeEventListener("codeterm:open-panel", opened);
  }
  expect(hasButton("Open Remote Friend's concierge")).toBe(true);
  expect(seen[seen.length - 1]?.has("tab-gateway")).toBe(true);
  expect(seen[seen.length - 1]?.has("tab-general")).toBe(false);
  expect(localClient.list).not.toHaveBeenCalled();
  expect(localRuntime.status).not.toHaveBeenCalled();
  expect(hasButton("Actions for Remote Friend's concierge")).toBe(false);
  expect(transport.webJson).toHaveBeenCalledWith("GET", "/api/neighbors/sidebar", undefined, { signal: expect.any(AbortSignal) });
  await act(async () => { button("Actions for Remote Friend").dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 })); });
  expect(Array.from(document.querySelectorAll('[role="menuitem"]')).map(item => item.textContent)).toEqual(["Remote Friend's settings"]);
  await act(async () => { document.querySelector<HTMLElement>('[role="menuitem"]')!.click(); });
  expect(takeNeighborFocus()).toEqual({ id: "pair", section: null });
  transport.webJson.mockResolvedValue({ pair: { tab_id: "tab-gateway", friend_display_name: "Remote Friend", friend_photo_url: null, paused: true, access: "paid" } });
  await act(async () => emit("neighbors-changed"));
  expect(button("Open Remote Friend").querySelector("img")).toBeNull();
  expect(button("Open Remote Friend").textContent).toContain("RF");
  expect(button("Open Remote Friend").textContent).toContain("disabled");
  transport.webJson.mockReturnValue(new Promise(() => {}));
  await act(async () => {
    window.dispatchEvent(new CustomEvent("codeterm-peer-switch-reset"));
    setRelayTarget("eight", "Eight");
    window.dispatchEvent(new CustomEvent("codeterm-peer-switch-rebind"));
  });
  expect(hasButton("Open Remote Friend's concierge")).toBe(false);
  expect(hasButton("Open Remote Friend")).toBe(false);
  expect(seen[seen.length - 1]?.size).toBe(0);
});

it("keeps the sidebar Neighbors section away until there is a Neighbor", async () => {
  await renderSection([]);
  expect(document.querySelector('section[aria-label="Neighbors"]')).toBeNull();
});

it("updates the sidebar from an account push and removes the shared subscription on account reset", async () => {
  await renderSection([]);
  const list = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  await act(async () => { emit("neighbors-changed", { uid: "owner", list }); });
  await expandSection();
  expect(hasButton("Open Ada")).toBe(true);
  await act(async () => mounted.current!.root.unmount());
  resetNeighborReads();
  expect(bridge.handlers["neighbors-changed"]).toBeUndefined();
  mounted.current!.host.remove();
  mounted.current = null;
});

it("nests each concierge under its Neighbor; the Neighbor opens your conversation and the concierge its chat", async () => {
  await renderSection([{ ...neighbor, photoURL: "https://example.test/ada.png" }, { ...neighbor, uid: "peer-2", displayName: "Lin", friendship_id: "pair-2" }], { pair: "tab-gateway" });
  expect(button("Open Ada").querySelector("img")?.getAttribute("src")).toBe("https://example.test/ada.png");
  expect(button("Open Lin").textContent).toContain("L");
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

it("offers the concierge actions: change agent, view permissions, edit with General and disable", async () => {
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
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<><SidebarNeighborsSection touchMode={false} client={client} runtimeClient={runtime()} /><Probe /></>));
  await expandSection();
  expect(hasButton("Open Ada's concierge")).toBe(true);
  expect(seen[seen.length - 1]?.has("tab-gateway")).toBe(true);
});

it("draws last known friends at once with a quiet refresh indicator, and stays away when there were none", async () => {
  localStorage.setItem("domios.neighbors.last-known:owner", JSON.stringify({ access: "paid", friends: [{ friendship_id: "pair", uid: "peer", displayName: "Ada", photoURL: null, gateway_tab: "tab-gateway" }] }));
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<SidebarNeighborsSection touchMode={false} client={pendingClient()} runtimeClient={runtime()} />));
  await expandSection();
  expect(hasButton("Open Ada")).toBe(true);
  expect(hasButton("Open Ada's concierge")).toBe(true);
  expect(document.querySelector('[aria-label="Refreshing Neighbors"]')).not.toBeNull();
  localStorage.setItem("domios.neighbors.last-known:owner", JSON.stringify({ access: "paid", friends: [] }));
  await act(async () => root.render(<SidebarNeighborsSection key="empty" touchMode={false} client={pendingClient()} runtimeClient={runtime()} />));
  await expandSection();
  expect(document.querySelector('section[aria-label="Neighbors"]')).toBeNull();
});

describe("the header's slot before its data arrives", () => {
  const signedIn = account.model;
  const lastKnown = (friends: unknown[]) => localStorage.setItem("domios.neighbors.last-known:owner", JSON.stringify({ access: "paid", friends }));
  const ada = { friendship_id: "pair", uid: "peer", displayName: "Ada", photoURL: null };
  const section = () => document.querySelector('section[aria-label="Neighbors"]');
  async function mount(client: NeighborClient) {
    const host = document.createElement("div"); document.body.append(host);
    const root = createRoot(host); mounted.current = { root, host };
    await act(async () => root.render(<SidebarNeighborsSection touchMode={false} client={client} runtimeClient={runtime()} />));
    return root;
  }
  afterEach(() => { account.model = signedIn; });

  it("holds it from the last known friends while the account is still being read", async () => {
    account.model = { kind: "pending" };
    lastKnown([ada]);
    const client = pendingClient();
    await mount(client);
    expect(section()).not.toBeNull();
    expect(document.querySelector('[aria-label="Refreshing Neighbors"]')).not.toBeNull();
    expect(client.list).not.toHaveBeenCalled();
  });

  it("stays the same header once the account and the list arrive", async () => {
    account.model = { kind: "pending" };
    lastKnown([ada]);
    const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
    const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}) };
    const root = await mount(client);
    const header = section();
    account.model = signedIn;
    await act(async () => root.render(<SidebarNeighborsSection touchMode={false} client={client} runtimeClient={runtime()} />));
    expect(section()).toBe(header);
    expect(document.querySelector('[aria-label="Refreshing Neighbors"]')).toBeNull();
  });

  it("reserves it for a signed-in account with no record yet, and gives it back when the list is empty", async () => {
    const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [], incoming: [], outgoing: [] };
    let release!: (value: NeighborList) => void;
    const client: NeighborClient = { list: vi.fn(() => new Promise<NeighborList>(resolve => { release = resolve; })), apply: vi.fn(async () => {}) };
    await mount(client);
    expect(section()).not.toBeNull();
    await act(async () => release(list));
    expect(section()).toBeNull();
  });

  it("stays away while the account is read when the last known list was empty, and when there is no record", async () => {
    account.model = { kind: "pending" };
    await mount(pendingClient());
    expect(section()).toBeNull();
    await act(async () => mounted.current!.root.unmount());
    mounted.current!.host.remove();
    lastKnown([]);
    await mount(pendingClient());
    expect(section()).toBeNull();
  });

  it("drops it when the account turns out to be signed out", async () => {
    account.model = { kind: "pending" };
    lastKnown([ada]);
    const root = await mount(pendingClient());
    expect(section()).not.toBeNull();
    account.model = { kind: "signed_out" };
    await act(async () => root.render(<SidebarNeighborsSection touchMode={false} client={pendingClient()} runtimeClient={runtime()} />));
    expect(section()).toBeNull();
    expect(localStorage.getItem("domios.neighbors.last-known:owner")).toBeNull();
  });
});

it("highlights an open concierge chat like an open tab, without the friend row", async () => {
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [{ ...neighbor, gateway_tab: "tab-gateway" }], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  const runtimeClient = runtime();
  await act(async () => root.render(<SidebarNeighborsSection touchMode={false} client={client} runtimeClient={runtimeClient} activeTabId="tab-gateway" />));
  await expandSection();
  expect(button("Open Ada's concierge").getAttribute("aria-current")).toBe("page");
  expect(button("Open Ada's concierge").closest("[data-tab-active]")?.querySelector(".active-tab-pill")).not.toBeNull();
  expect(button("Open Ada").getAttribute("aria-current")).toBeNull();
  await act(async () => root.render(<SidebarNeighborsSection touchMode={false} client={client} runtimeClient={runtimeClient} activeTabId="tab-general" />));
  await expandSection();
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
  await act(async () => mounted.current!.root.render(<SidebarNeighborsSection touchMode={false} runtimeClient={runtime()} />));
  await expandSection();
  expect(localStorage.getItem("domios.neighbors.last-known:owner")).toBeNull();
});

it("never claims a missing concierge when one is bound, and omits a zero count", async () => {
  expect(missingConciergeLine({ gateway_tab: null, concierge_missing: true, pending_count: 0, receiving_here: true, receiver_machine_label: null, state: "concierge_missing" }))
    .toBe("No concierge on this machine");
  const runtimeClient = runtime({ pair: "tab-gateway" });
  vi.mocked(runtimeClient.status).mockResolvedValue({ enrolled: true, bindings: { pair: "tab-gateway" },
    concierges: { pair: { gateway_tab: null, concierge_missing: true, pending_count: 2, receiving_here: true, receiver_machine_label: null, state: "concierge_missing" } } } as unknown as Awaited<ReturnType<NeighborRuntimeClient["status"]>>);
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<SidebarNeighborsSection touchMode={false} client={client} runtimeClient={runtimeClient} />));
  await expandSection();
  expect(hasButton("Open Ada's concierge")).toBe(true);
  expect(document.body.textContent).not.toContain("no concierge here");
});

it("disables a friend from their sidebar row menu and shows the row as disabled", async () => {
  const conciergeClient = concierge([]);
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, incoming: [], outgoing: [], friends: [neighbor] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<SidebarNeighborsSection touchMode={false} client={client} runtimeClient={runtime()} conciergeClient={conciergeClient} />));
  await expandSection();
  await act(async () => { button("Actions for Ada").dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 })); });
  await act(async () => Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(item => item.textContent === "Disable Ada")!.click());
  expect(conciergeClient.setPaused).toHaveBeenCalledExactlyOnceWith("pair", true);
  expect(button("Open Ada").textContent).toContain("disabled");
});
it("ends a refused first read without claiming cached friends are connected", async () => {
  localStorage.setItem("domios.neighbors.last-known:owner", JSON.stringify({ access: "paid", friends: [neighbor] }));
  const client: NeighborClient = { list: vi.fn(async () => { throw new Error("Sign in with a hosted account to use Neighbors"); }), apply: vi.fn() };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<SidebarNeighborsSection touchMode={false} client={client} runtimeClient={runtime()} />));
  await expandSection();
  expect(document.querySelector('[aria-label="Refreshing Neighbors"]')).toBeNull();
  expect(host.querySelector('[role="status"]')?.textContent).toContain("Sign in with a hosted account to use Neighbors");
  expect(host.textContent).not.toContain("connected");
});
