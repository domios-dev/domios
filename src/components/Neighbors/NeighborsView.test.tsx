import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { NeighborsView } from "./NeighborsView";
import { SidebarNeighborsSection } from "./SidebarNeighborsSection";
import { NeighborConversationView } from "./NeighborConversationView";
import type { HistoryEntry, NeighborConciergeClient } from "./neighborConciergeClient";
import { takeNeighborFocus } from "./neighborFocus";
import type { NeighborClient } from "./neighborClient";
import type { NeighborList } from "./neighborModel";
import { parseNeighbors } from "./neighborModel";
import { parseReferralClaim, type ReferralClaim } from "./referralTrial";
import { neighborClient } from "./neighborClient";
import type { NeighborRuntimeClient } from "./neighborRuntimeClient";
import { localFetch } from "../../lib/webApi";

const notices = vi.hoisted(() => ({ publishNotice: vi.fn() }));
const transport = vi.hoisted(() => ({ localFetch: vi.fn() }));
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
vi.mock("../../store/multiplexer", () => ({ useMuxSelector: (select: (state: typeof mux) => unknown) => select(mux) }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const neighbor = { uid: "peer", displayName: "Ada", photoURL: null, friendship_id: "pair", inbound_per_hour: 10, outbound_per_hour: 20, receiving: true, since: 1700000000 };
const request = { uid: "requester", displayName: "Grace", photoURL: null, request_id: "request", created_at: 1700000000 };
const signedIn = account.model;
let mounted: { root: ReturnType<typeof createRoot>; host: HTMLDivElement } | null = null;
afterEach(async () => {
  if (mounted) { await act(async () => mounted!.root.unmount()); mounted.host.remove(); mounted = null; }
  localStorage.clear();
  notices.publishNotice.mockClear();
  transport.localFetch.mockReset();
  bridge.invoke.mockClear();
  account.model = signedIn;
});

function button(label: string): HTMLButtonElement {
  const found = Array.from(document.querySelectorAll("button")).find(element => element.getAttribute("aria-label") === label || element.textContent === label);
  if (!found) throw new Error(`Missing button ${label}`);
  return found;
}
const hasButton = (label: string) => Array.from(document.querySelectorAll("button")).some(element => element.getAttribute("aria-label") === label || element.textContent === label);

function emit(event: string, payload: unknown = null) {
  for (const handler of bridge.handlers[event] ?? []) handler({ payload });
}

function runtime(bindings: Record<string, string> = {}): NeighborRuntimeClient {
  return {
    subscribe: vi.fn(async () => () => {}), status: vi.fn(async () => ({ enrolled: false, bindings, receiving_here: false, receiving_enabled: false, receiver_machine_label: null, revision: 0 })), enroll: vi.fn(async () => {}), rotate: vi.fn(async () => {}),
    permission: vi.fn(async () => ({ allowed: false, scope: "", allow_tab_requests: false })), send: vi.fn(), deliveryStatus: vi.fn(), retry: vi.fn(async () => {}),
  };
}

async function render(access: NeighborList["access"] = "paid", options: { canInvite?: boolean; omitEligibility?: boolean; claim?: ReferralClaim; bindings?: Record<string, string>; outgoing?: NeighborList["outgoing"]; runtimeClient?: NeighborRuntimeClient } = {}) {
  const list: NeighborList = { schema_version: 1, access, can_invite: options.omitEligibility ? undefined : options.canInvite ?? access === "paid", friends: [neighbor], incoming: [request], outgoing: options.outgoing ?? [] };
  const client: NeighborClient = { list: vi.fn(async () => structuredClone(list)), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => options.claim ?? { schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null }) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted = { root, host };
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={options.runtimeClient ?? runtime(options.bindings)} />));
  return { client, list };
}

const sentText = () => bridge.invoke.mock.calls.map(call => (call as unknown as [string, { tabId: string; text: string }])[1]);

it("draws you, each Neighbor and request without duplicating daemon notifications", async () => {
  const { client } = await render();
  expect(hasButton("Ada, connected")).toBe(true);
  expect(hasButton("Grace, wants to connect")).toBe(true);
  expect(notices.publishNotice).not.toHaveBeenCalled();
  await act(async () => button("Refresh Neighbors").click());
  expect(client.list).toHaveBeenCalledTimes(2);
  expect(notices.publishNotice).not.toHaveBeenCalled();
});

it("updates the graph from an own-account push and ignores foreign or stale loads", async () => {
  const { client, list } = await render();
  let finish!: (value: NeighborList) => void;
  vi.mocked(client.list).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  await act(async () => button("Refresh Neighbors").click());
  const newer = { ...list, friends: [{ ...neighbor, displayName: "Lin" }], incoming: [] };
  await act(async () => { emit("neighbors-changed", { uid: "other", list: newer }); });
  expect(hasButton("Ada, connected")).toBe(true);
  await act(async () => { emit("neighbors-changed", { uid: "owner", list: newer }); });
  expect(hasButton("Lin, connected")).toBe(true);
  expect(hasButton("Grace, wants to connect")).toBe(false);
  await act(async () => finish(list));
  expect(hasButton("Lin, connected")).toBe(true);
});

it("saves an email request once, then starts the permission interview in the General Agent", async () => {
  const { client } = await render();
  const focused = vi.fn();
  window.addEventListener("codeterm:focus-tab", focused);
  try {
    await act(async () => button("Add Neighbor").click());
    const input = document.querySelector<HTMLInputElement>('input[type="email"]')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "friend@example.test");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => document.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })));
    expect(client.apply).toHaveBeenCalledExactlyOnceWith({ operation: "request", email: "friend@example.test" });
    expect(sentText()).toHaveLength(1);
    expect(sentText()[0].tabId).toBe("tab-general");
    expect(sentText()[0].text).toContain("friend@example.test");
    expect((focused.mock.calls[0][0] as CustomEvent).detail).toEqual({ tabId: "tab-general", workspaceId: "ws-general" });
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  } finally { window.removeEventListener("codeterm:focus-tab", focused); }
});

it("accepts a request and interviews about the new connection once it exists", async () => {
  const { client, list } = await render("locked", { canInvite: false });
  expect(hasButton("Upgrade to add Neighbors")).toBe(true);
  await act(async () => button("Grace, wants to connect").click());
  list.incoming = [];
  list.friends = [neighbor, { ...neighbor, uid: "requester", displayName: "Grace", friendship_id: "pair-grace" }];
  await act(async () => button("Accept Grace").click());
  expect(client.apply).toHaveBeenCalledWith({ operation: "respond", request_id: "request", accept: true });
  expect(sentText()).toHaveLength(1);
  expect(sentText()[0].text).toContain("pair-grace");
});

it("manages a Neighbor from its panel: limit, talk about, and revoke only after confirmation", async () => {
  const { client } = await render();
  await act(async () => button("Ada, connected").click());
  await act(async () => button("Configure").click());
  const input = Array.from(document.querySelectorAll<HTMLLabelElement>("label")).find(label => label.textContent === "Incoming requests per hour")!.control as HTMLInputElement;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "42");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => button("Save limit").click());
  expect(client.apply).toHaveBeenCalledWith({ operation: "set_limit", friendship_id: "pair", per_hour: 42 });
  await act(async () => button("Talk about Ada").click());
  expect(sentText()[0].text).toContain("pair");
  await act(async () => button("Revoke").click());
  expect(client.apply).not.toHaveBeenCalledWith({ operation: "remove", friendship_id: "pair" });
  await act(async () => button("Confirm revoke").click());
  expect(client.apply).toHaveBeenCalledWith({ operation: "remove", friendship_id: "pair" });
});

it("opens the gateway tab only when one receives this Neighbor", async () => {
  await render("paid", { bindings: { pair: "tab-gateway" } });
  const focused = vi.fn();
  window.addEventListener("codeterm:focus-tab", focused);
  try {
    await act(async () => button("Ada, connected").click());
    await act(async () => button("Open their tab").click());
    expect((focused.mock.calls[0][0] as CustomEvent).detail).toEqual({ tabId: "tab-gateway", workspaceId: "ws-work" });
  } finally { window.removeEventListener("codeterm:focus-tab", focused); }
});

it.each([false, undefined])("requires explicit server invitation permission, even with paid channel access (%s)", async canInvite => {
  const { client } = await render("paid", { canInvite, omitEligibility: canInvite === undefined });
  expect(document.querySelector('input[type="email"]')).toBeNull();
  if (canInvite === undefined) expect(button("Add Neighbor").disabled).toBe(true);
  await act(async () => button("Grace, wants to connect").click());
  await act(async () => button("Accept Grace").click());
  expect(client.apply).toHaveBeenCalledExactlyOnceWith({ operation: "respond", request_id: "request", accept: true });
});

it("consumes an idempotent referral claim before listing requests without opening checkout", async () => {
  const { client } = await render("paid", { canInvite: false, claim: {
    schema_version: 1, outcome: "already_granted", trial_end: 1700604800, friend_request_id: "a".repeat(64),
  } });
  expect(client.claimReferralTrial).toHaveBeenCalledTimes(1);
  expect(vi.mocked(client.claimReferralTrial).mock.invocationCallOrder[0]).toBeLessThan(vi.mocked(client.list).mock.invocationCallOrder[0]);
  expect(button("Subscribe")).toBeDefined();
  expect(client.apply).not.toHaveBeenCalled();
  await act(async () => button("Refresh Neighbors").click());
  expect(client.claimReferralTrial).toHaveBeenCalledTimes(2);
  expect(client.apply).not.toHaveBeenCalled();
});

it("keeps a server-locked Neighbor manageable, offers plans and never unlocks the channel", async () => {
  const { client } = await render("locked", { canInvite: false, claim: {
    schema_version: 1, outcome: "granted", trial_end: 4102444800, friend_request_id: null,
  } });
  await act(async () => button("Ada, paused by plan").click());
  expect(button("View plans")).toBeDefined();
  await act(async () => button("Messages").click());
  expect(hasButton("Enable encrypted channel")).toBe(false);
  expect(client.apply).not.toHaveBeenCalled();
});

it("refreshes the server's channel lock when signed entitlement access changes", async () => {
  const { list } = await render("paid", { canInvite: false, claim: {
    schema_version: 1, outcome: "already_granted", trial_end: 1700604800, friend_request_id: null,
  } });
  await act(async () => button("Ada, connected").click());
  expect(hasButton("View plans")).toBe(false);
  list.access = "locked";
  expect(bridge.handlers["entitlement-changed"]).toBeDefined();
  await act(async () => emit("entitlement-changed"));
  expect(button("View plans")).toBeDefined();
  expect(button("Subscribe")).toBeDefined();
});

it("discards a claim that completes after switching accounts", async () => {
  let complete!: (claim: ReferralClaim) => void;
  const previous = new Promise<ReferralClaim>(resolve => { complete = resolve; });
  const client: NeighborClient = {
    claimReferralTrial: vi.fn<NeighborClient["claimReferralTrial"]>().mockReturnValueOnce(previous).mockResolvedValue({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null }),
    list: vi.fn(async () => ({ schema_version: 1 as const, access: "locked" as const, can_invite: false, friends: [], incoming: [], outgoing: [] })),
    apply: vi.fn(async () => {}),
  };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted = { root, host };
  account.model = { kind: "signed_in", identity: { uid: "previous", email: null, display_name: "Previous", photo_url: null } };
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={runtime()} />));
  account.model = signedIn;
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={runtime()} />));
  await act(async () => complete({ schema_version: 1, outcome: "granted", trial_end: 1700604800, friend_request_id: "a".repeat(64) }));
  expect(client.list).toHaveBeenCalledTimes(1);
  expect(hasButton("Subscribe")).toBe(false);
});

it("offers sign-in at the center when signed out, without making an invitation", async () => {
  account.model = { kind: "signed_out" };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted = { root, host };
  await act(async () => root.render(<NeighborsView runtimeClient={runtime()} />));
  expect(hasButton("Add Neighbor")).toBe(false);
  await act(async () => button("Sign in or create account").click());
  expect(account.signIn).toHaveBeenCalledTimes(1);
  expect(localFetch).not.toHaveBeenCalled();
});

it("rejects unrecognized or malformed claims and never substitutes channel access for invite permission", () => {
  for (const value of [
    { schema_version: 2, outcome: "granted", trial_end: 1700604800, friend_request_id: null },
    { schema_version: 1, outcome: "granted", trial_end: null, friend_request_id: null },
    { schema_version: 1, outcome: "not_invited", trial_end: 1700604800, friend_request_id: null },
    { schema_version: 1, outcome: "unknown", trial_end: null, friend_request_id: null },
    { schema_version: 1, outcome: "granted", trial_end: 1700604800, friend_request_id: "foreign" },
  ]) expect(() => parseReferralClaim(value)).toThrow();
  expect(parseNeighbors({ schema_version: 1, access: "paid", friends: [], incoming: [], outgoing: [] }).can_invite).toBeUndefined();
  expect(() => parseNeighbors({ schema_version: 1, access: "paid", can_invite: "yes", friends: [], incoming: [], outgoing: [] })).toThrow();
});

it("sends an empty authenticated referral command without user IDs, checkout or a local deadline", async () => {
  const response = { schema_version: 1, outcome: "granted", trial_end: 1700604800, friend_request_id: null };
  vi.mocked(localFetch).mockResolvedValue(new Response(JSON.stringify(response), { status: 200 }));
  expect(await neighborClient.claimReferralTrial()).toEqual(response);
  expect(localFetch).toHaveBeenCalledTimes(1);
  const [path, init] = vi.mocked(localFetch).mock.calls[0];
  expect(path).toBe("/api/account/neighbors");
  expect(JSON.parse(String(init?.body))).toEqual({ operation: "claim_referral_trial" });
});

async function renderSection(friends: NeighborList["friends"], bindings: Record<string, string> = {}) {
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends, incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted = { root, host };
  await act(async () => root.render(<SidebarNeighborsSection touchMode={false} client={client} runtimeClient={runtime(bindings)} />));
}

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

it("offers the concierge actions: change agent, view permissions and edit with General", async () => {
  await renderSection([neighbor], { pair: "tab-gateway" });
  await act(async () => { button("Actions for Ada's concierge").dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 })); });
  const items = Array.from(document.querySelectorAll('[role="menuitem"]')).map(item => item.textContent);
  expect(items).toEqual(["Change agent", "View permissions", "Edit with General"]);
  const viewPermissions = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(item => item.textContent === "View permissions")!;
  await act(async () => viewPermissions.click());
  expect(takeNeighborFocus()).toEqual({ id: "pair", section: "configure" });
});

it("saves permissions directly and refuses an allowed grant without a scope", async () => {
  const conciergeClient = { history: vi.fn(), onHistoryChanged: vi.fn(async () => () => {}), onPermissionChanged: vi.fn(async () => () => {}),
    onAgentChanged: vi.fn(async () => () => {}), setPermission: vi.fn(async () => {}), changeAgent: vi.fn(),
    setDailyLimit: vi.fn(async () => {}), onDailyLimitChanged: vi.fn(async () => () => {}) };
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted = { root, host };
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={runtime()} conciergeClient={conciergeClient} />));
  await act(async () => button("Ada, connected").click());
  await act(async () => button("Configure").click());
  const allow = document.querySelector<HTMLButtonElement>('button[aria-labelledby][role="switch"], .neighbor-permissions button[aria-labelledby]')!;
  await act(async () => allow.click());
  expect(button("Save permissions").disabled).toBe(true);
  const scope = document.querySelector<HTMLTextAreaElement>(".neighbor-permissions textarea")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(scope, "Questions about our shared project");
    scope.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => button("Save permissions").click());
  expect(conciergeClient.setPermission).toHaveBeenCalledExactlyOnceWith("pair", { allowed: true, scope: "Questions about our shared project", allow_tab_requests: false });
});

it("shows a sent invitation as its typed email with no profile and no action", async () => {
  const { client } = await render("paid", { outgoing: [{ id: "b".repeat(64), typed_email: "new@example.test", created_at: 1700000000, state: "pending" }] });
  const node = Array.from(document.querySelectorAll<HTMLElement>('[data-status="invited"]'));
  expect(node).toHaveLength(1);
  expect(node[0].tagName).toBe("DIV");
  expect(node[0].textContent).toContain("new@example.test");
  expect(node[0].querySelector("img")).toBeNull();
  expect(hasButton("new@example.test")).toBe(false);
  expect(client.apply).not.toHaveBeenCalled();
});

it("follows the daemon's pushed list for this account and ignores another account's", async () => {
  const { list } = await render();
  const pushed = { ...list, friends: [], incoming: [], outgoing: [{ id: "c".repeat(64), typed_email: "pushed@example.test", created_at: 1700000000, state: "pending" as const }] };
  await act(async () => emit("neighbors-changed", { uid: "someone-else", list: pushed }));
  expect(hasButton("Ada, connected")).toBe(true);
  await act(async () => emit("neighbors-changed", { uid: "owner", list: pushed }));
  expect(hasButton("Ada, connected")).toBe(false);
  expect(document.querySelector('[data-status="invited"]')?.textContent).toContain("pushed@example.test");
});

it("shows a calm off state with a way back on, and draws no ring while Neighbors is off", async () => {
  vi.mocked(localFetch).mockResolvedValue(new Response(JSON.stringify({ ok: true, enabled: true }), { status: 200 }));
  const client: NeighborClient = { list: vi.fn(async () => ({ schema_version: 1 as const, access: "paid" as const, can_invite: true, friends: [neighbor], incoming: [], outgoing: [] })),
    apply: vi.fn(async () => {}), claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted = { root, host };
  await act(async () => root.render(<NeighborsView config={{ neighbors: { enabled: false } }} client={client} runtimeClient={runtime()} />));
  expect(hasButton("Ada, connected")).toBe(false);
  expect(hasButton("Add Neighbor")).toBe(false);
  await act(async () => button("Turn on Neighbors").click());
  const [path, init] = vi.mocked(localFetch).mock.calls[0];
  expect(path).toBe("/api/account/neighbors/enabled");
  expect(JSON.parse(String(init?.body))).toEqual({ enabled: true });
  expect(hasButton("Ada, connected")).toBe(true);
});

function concierge(entries: HistoryEntry[]): NeighborConciergeClient & { changed: (friendship: string) => void } {
  let changed = (_friendship: string) => {};
  return {
    history: vi.fn(async () => ({ entries: [...entries].reverse(), next_before: null, ended: true })),
    onHistoryChanged: vi.fn(async (handler: (friendship: string) => void) => { changed = handler; return () => {}; }),
    onPermissionChanged: vi.fn(async () => () => {}), onAgentChanged: vi.fn(async () => () => {}),
    setPermission: vi.fn(async () => {}), changeAgent: vi.fn(async () => "tab-gateway"),
    setDailyLimit: vi.fn(async () => {}), onDailyLimitChanged: vi.fn(async () => () => {}),
    changed: (friendship: string) => changed(friendship),
  };
}

it("shows the conversation with a refused question, its linked refusal and delivery, and sends straight to the Neighbor", async () => {
  const entries: HistoryEntry[] = [
    { direction: "in", kind: "question", author: "friend", body: "Can you share the deploy keys?", at: 1700000000000, message_id: "m1", outcome: "refused", reason: "Secrets are off limits" },
    { direction: "out", kind: "refusal", author: { agent: "tab-gateway", name: "Concierge" }, body: "I can't share secrets.", at: 1700000060000, message_id: "m2", in_reply_to: "m1", delivery_status: "delivered" },
  ];
  const conciergeClient = concierge(entries);
  const runtimeClient = runtime();
  vi.mocked(runtimeClient.send).mockResolvedValue({ message_id: "f".repeat(32), status: "pending" });
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={client} runtimeClient={runtimeClient} conciergeClient={conciergeClient} />));
  const log = document.querySelector('[role="log"]')!;
  expect(log.textContent).toContain("Can you share the deploy keys?");
  expect(log.textContent).toContain("refused: Secrets are off limits");
  expect(document.getElementById("neighbor-msg-m2")?.textContent).toContain("Can you share the deploy keys?");
  expect(log.textContent).toContain("delivered");
  const input = document.querySelector<HTMLTextAreaElement>("textarea")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(input, "Lunch on Friday?");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => button("Send").click());
  expect(runtimeClient.send).toHaveBeenCalledExactlyOnceWith("pair", "Lunch on Friday?");
  expect(conciergeClient.history).toHaveBeenCalledTimes(2);
  await act(async () => conciergeClient.changed("other-pair"));
  expect(conciergeClient.history).toHaveBeenCalledTimes(2);
  await act(async () => conciergeClient.changed("pair"));
  expect(conciergeClient.history).toHaveBeenCalledTimes(3);
});

it("shows their daily allowance, explains a limit refusal and holds the composer once the day is used up", async () => {
  const entries: HistoryEntry[] = [
    { direction: "out", kind: "question", author: "me", body: "One more thing", at: 1700000000000, message_id: "m9",
      delivery_status: "not_delivered", reason: "neighbor_daily_limit", limit: 3, resets_at: 4102444800000 },
  ];
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, outgoing: [], incoming: [],
    friends: [{ ...neighbor, their_daily_limit: 3, sent_today: 3, my_daily_limit: null }] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={client} runtimeClient={runtime()} conciergeClient={concierge(entries)} />));
  expect(document.body.textContent).toContain("3/day, 3 sent today");
  expect(document.getElementById("neighbor-msg-m9")?.textContent).toContain("Limit reached: Ada allows 3 messages a day; ask them to raise it.");
  expect(document.querySelector<HTMLTextAreaElement>("textarea")!.disabled).toBe(true);
});

it("tells the old machine that receiving moved, and warns before taking it over", async () => {
  const runtimeClient = runtime();
  vi.mocked(runtimeClient.status).mockResolvedValue({ enrolled: true, bindings: { pair: "tab-gateway" }, receiving_here: false, receiving_enabled: true, receiver_machine_label: "Studio", revision: 4 });
  await render("paid", { runtimeClient });
  expect(document.body.textContent).toContain("Receiving moved to Studio");
  await act(async () => button("Ada, connected").click());
  await act(async () => button("Messages").click());
  await act(async () => button("Receive here").click());
  expect(document.querySelector('[aria-label="Receive on this machine"]')?.textContent)
    .toContain("Studio will stop receiving Neighbor messages. Concierges and permissions are per machine");
});
