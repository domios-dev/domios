import { it, expect, vi } from "vitest";
import { act } from "react";
import type { NeighborList } from "./neighborModel";
import type { ReferralClaim } from "./referralTrial";
import type { NeighborClient } from "./neighborClient";
import { createRoot } from "react-dom/client";
import { NeighborsView } from "./NeighborsView";
import { localFetch } from "../../lib/webApi";
import { parseReferralClaim } from "./referralTrial";
import { parseNeighbors } from "./neighborModel";
import { neighborClient } from "./neighborClient";
import type { NeighborNotify, HistoryEntry } from "./neighborConciergeClient";
import { NeighborRuntimeFailure, type NeighborListenerEvent, type NeighborRuntimeClient } from "./neighborRuntimeClient";
import { missingConciergeLine } from "./useConciergeStatus";
import { openNeighborPage } from "./neighborFocus";
import { createNeighborsViewFixture } from "./neighborsView.testdata";

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

const { neighbor, signedIn, mounted, button, hasButton, emit, runtime, render, sentText, concierge, pendingClient } = createNeighborsViewFixture({ notices, transport, bridge, account, mux });



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
  const commands = Array.from(sentText()[0].text.matchAll(/\x60([^\x60]+)\x60/g), match => match[1]);
  expect(commands).toContain("domios account neighbors grant pair-grace --from-pad <file>");
  expect(commands).toContain("--allow-tab-requests true");
});

it("opens a friend's own page: limit, talk about, and removal only after confirmation", async () => {
  const { client } = await render();
  await act(async () => button("Ada, connected").click());
  expect(document.querySelector('[data-settings-page="ada"]')).not.toBeNull();
  const input = Array.from(document.querySelectorAll<HTMLLabelElement>("label")).find(label => label.textContent === "Incoming requests per hour")!.control as HTMLInputElement;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "42");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => button("Apply").click());
  expect(client.apply).toHaveBeenCalledWith({ operation: "set_limit", friendship_id: "pair", per_hour: 42 });
  await act(async () => button("Talk about Ada").click());
  expect(sentText()[0].text).toContain("pair");
  await act(async () => button("Remove friend").click());
  expect(client.apply).not.toHaveBeenCalledWith({ operation: "remove", friendship_id: "pair" });
  await act(async () => button("Confirm remove").click());
  expect(client.apply).toHaveBeenCalledWith({ operation: "remove", friendship_id: "pair" });
});

it("opens the conversation and the concierge's chat from the friend page", async () => {
  await render("paid", { bindings: { pair: "tab-gateway" }, friend: { ...neighbor, gateway_tab: "tab-gateway" } });
  const focused = vi.fn();
  const panels = vi.fn();
  window.addEventListener("codeterm:focus-tab", focused);
  window.addEventListener("codeterm:open-panel", panels);
  try {
    await act(async () => button("Ada, connected").click());
    await act(async () => button("Open conversation").click());
    expect((panels.mock.calls[0][0] as CustomEvent).detail).toEqual({ panel: "neighbor:pair" });
    await act(async () => button("Open its chat").click());
    expect((focused.mock.calls[0][0] as CustomEvent).detail).toEqual({ tabId: "tab-gateway", workspaceId: "ws-work" });
  } finally {
    window.removeEventListener("codeterm:focus-tab", focused);
    window.removeEventListener("codeterm:open-panel", panels);
  }
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
  expect(client.claimReferralTrial).toHaveBeenCalledTimes(1);
  expect(client.apply).not.toHaveBeenCalled();
});

it("keeps a server-locked Neighbor manageable, offers plans and never unlocks the channel", async () => {
  const { client } = await render("locked", { canInvite: false, claim: {
    schema_version: 1, outcome: "granted", trial_end: 4102444800, friend_request_id: null,
  } });
  await act(async () => button("Ada, paused by plan").click());
  expect(button("View plans")).toBeDefined();
  expect(hasButton("Open conversation")).toBe(false);
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
  const root = createRoot(host); mounted.current = { root, host };
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
  const root = createRoot(host); mounted.current = { root, host };
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

it("changes the concierge with the shared agent row instead of bespoke dropdowns", async () => {
  await render("paid");
  await act(async () => button("Ada, connected").click());
  await act(async () => button("Set up concierge").click());
  expect(document.querySelector('[data-testid="orchestration-role-agent"]')).not.toBeNull();
  expect(document.querySelector('[role="dialog"] select')).toBeNull();
  expect(document.body.textContent).toContain("Change Ada's concierge");
});

it("saves permissions directly and refuses an allowed grant without a scope", async () => {
  const conciergeClient = { history: vi.fn(async () => ({ entries: [], next_before: null, ended: false })), onHistoryChanged: vi.fn(async () => () => {}), onPermissionChanged: vi.fn(async () => () => {}),
    onAgentChanged: vi.fn(async () => () => {}), setPermission: vi.fn(async () => {}), changeAgent: vi.fn(),
    setDailyLimit: vi.fn(async () => {}), onDailyLimitChanged: vi.fn(async () => () => {}),
    setNotify: vi.fn(async (_friendship: string, notify: NeighborNotify) => notify), onNotifyChanged: vi.fn(async () => () => {}),
    setPaused: vi.fn(async (_friendship: string, paused: boolean) => paused), onPausedChanged: vi.fn(async () => () => {}), stopConcierge: vi.fn(async () => {}) };
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={runtime()} conciergeClient={conciergeClient} />));
  await act(async () => button("Ada, connected").click());
  const allow = button("Allow requests from Ada");
  await act(async () => allow.click());
  expect(button("Apply").disabled).toBe(true);
  const scope = document.querySelector<HTMLTextAreaElement>(".neighbor-permissions textarea")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(scope, "Questions about our shared project");
    scope.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(document.body.textContent).toContain("Applying restarts Ada's concierge with a fresh session.");
  await act(async () => button("Apply").click());
  expect(conciergeClient.setPermission).toHaveBeenCalledExactlyOnceWith("pair", { allowed: true, scope: "Questions about our shared project", allow_tab_requests: false, memory_write_areas: [] });
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
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborsView config={{ neighbors: { enabled: false } }} client={client} runtimeClient={runtime()} />));
  expect(hasButton("Ada, connected")).toBe(false);
  expect(hasButton("Add Neighbor")).toBe(false);
  await act(async () => button("Turn on Neighbors").click());
  const [path, init] = vi.mocked(localFetch).mock.calls[0];
  expect(path).toBe("/api/account/neighbors/enabled");
  expect(JSON.parse(String(init?.body))).toEqual({ enabled: true });
  expect(hasButton("Ada, connected")).toBe(true);
});

it("tells the old machine that receiving moved, and warns before taking it over", async () => {
  const runtimeClient = runtime();
  vi.mocked(runtimeClient.status).mockResolvedValue({ enrolled: true, bindings: { pair: "tab-gateway" }, receiving_here: false, receiving_enabled: true, receiver_machine_label: "Studio", revision: 4 });
  await render("paid", { runtimeClient });
  expect(document.body.textContent).toContain("Receiving on Studio");
  await act(async () => button("Receive all friends here").click());
  expect(document.querySelector('[aria-label="Receive on this machine"]')?.textContent)
    .toContain("Studio will stop receiving Neighbor messages.");
  await act(async () => button("Receive all friends here").click());
  expect(runtimeClient.enroll).toHaveBeenCalledExactlyOnceWith(4);
});

it.each([
  ["receiver_revision_stale", "Receiving changed; refresh and confirm again", "Receiving changed; refresh and confirm again"],
  ["unavailable", "Neighbor request refused", "The receiving service is unavailable. Try again."],
  ["unavailable", 'Authorization: Bearer opaque-secret-response', "The receiving service is unavailable. Try again."],
  ["receiver_revision_stale", "Receiving changed\nAuthorization: Bearer opaque-secret-typed", "Receiving changed [redacted]"],
  ["receiver_revision_stale", "x".repeat(4096) + "opaque-secret-tail", "x".repeat(255) + "…"],
] as const)("shows safe takeover failure %s without expanding Details", async (reason, message, displayed) => {
  const runtimeClient = runtime();
  vi.mocked(runtimeClient.status).mockResolvedValue({ enrolled: false, bindings: {}, receiving_here: false, receiving_enabled: true, receiver_machine_label: "Studio", revision: 4 });
  vi.mocked(runtimeClient.enroll).mockRejectedValue(new NeighborRuntimeFailure(reason, message));
  await render("paid", { runtimeClient });
  await act(async () => button("Receive all friends here").click());
  await act(async () => button("Receive all friends here").click());
  const alert = document.querySelector('[role="alert"]');
  const diagnostic = Array.from(alert?.querySelectorAll("*") ?? [])
    .find(node => node.textContent === `${reason}: ${displayed}` && !node.closest("details"));
  expect(diagnostic).toBeDefined();
  expect(alert?.querySelector("details")?.open).toBe(false);
  expect(alert?.querySelector("pre")?.textContent).toBe(`${reason}: ${displayed}`);
  expect(alert?.textContent).not.toContain("opaque-secret");
});

it.each([
  new Error("opaque-secret-error"),
  { token: "opaque-secret-object", message: "opaque-secret-message" },
])("does not retain non-runtime takeover rejection %# in the alert or Details", async failure => {
  const runtimeClient = runtime();
  vi.mocked(runtimeClient.status).mockResolvedValue({ enrolled: false, bindings: {}, receiving_here: false, receiving_enabled: true, receiver_machine_label: "Studio", revision: 4 });
  vi.mocked(runtimeClient.enroll).mockRejectedValue(failure);
  await render("paid", { runtimeClient });
  await act(async () => button("Receive all friends here").click());
  await act(async () => button("Receive all friends here").click());
  const alert = document.querySelector('[role="alert"]');
  expect(alert?.textContent).toContain("Receiving could not move here. Try again.");
  expect(alert?.textContent).not.toContain("opaque-secret");
  expect(alert?.querySelector("pre")?.textContent).toBe("Receiving could not move here. Try again.");
});

it("requires a fresh confirmation if receiving moves while the active takeover prompt is open", async () => {
  const runtimeClient = runtime();
  let changed: ((event: NeighborListenerEvent) => void) | undefined;
  vi.mocked(runtimeClient.subscribe).mockImplementation(async handler => { changed = handler; return () => {}; });
  vi.mocked(runtimeClient.status).mockResolvedValue({ enrolled: true, bindings: {}, receiving_here: false, receiving_enabled: true, receiver_machine_label: "Studio", revision: 4 });
  await render("paid", { runtimeClient });
  await act(async () => button("Receive all friends here").click());
  await act(async () => changed?.({ receiving_here: false, receiving_enabled: true, receiver_machine_label: "Laptop", revision: 5 }));
  await act(async () => button("Receive all friends here").click());
  expect(runtimeClient.enroll).not.toHaveBeenCalled();
  await act(async () => button("Receive all friends here").click());
  expect(runtimeClient.enroll).toHaveBeenCalledExactlyOnceWith(5);
});

it("draws the last known graph at once and says it is updating instead of loading", async () => {
  localStorage.setItem("domios.neighbors.last-known:owner", JSON.stringify({ access: "paid", can_invite: true,
    friends: [{ friendship_id: "pair", uid: "peer", displayName: "Ada", photoURL: null }],
    incoming: [{ request_id: "request", uid: "requester", displayName: "Grace", photoURL: null }],
    outgoing: [{ id: "c".repeat(64), typed_email: "later@example.test", created_at: 1700000000 }] }));
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborsView client={pendingClient()} runtimeClient={runtime()} />));
  expect(hasButton("Ada, connected")).toBe(true);
  expect(hasButton("Grace, wants to connect")).toBe(true);
  expect(document.querySelector('[data-status="invited"]')?.textContent).toContain("later@example.test");
  expect(document.body.textContent).toContain("Updating");
  expect(document.body.textContent).not.toContain("Loading Neighbors");
});

it("caps the waiting count the way the daemon does", () => {
  expect(missingConciergeLine({ gateway_tab: null, concierge_missing: true, pending_count: 100, receiving_here: true, receiver_machine_label: null, state: "concierge_missing" }))
    .toBe("No concierge on this machine — 100+ messages waiting");
});

it("says nothing about receiving until the daemon knows", async () => {
  const runtimeClient = runtime();
  vi.mocked(runtimeClient.status).mockResolvedValue({ enrolled: true, bindings: {}, receiving_here: false, receiving_enabled: false, receiver_machine_label: null, revision: 0,
    receiver_status_known: false } as unknown as Awaited<ReturnType<NeighborRuntimeClient["status"]>>);
  await render("paid", { runtimeClient });
  expect(document.body.textContent).not.toContain("Receiver not chosen yet");
  expect(hasButton("Receive all friends here")).toBe(false);
});

it("turns off reading memory and files for one friend and saves it", async () => {
  const conciergeClient = concierge([]);
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={runtime()} conciergeClient={conciergeClient} />));
  await act(async () => button("Ada, connected").click());
  expect(button("Read my memory").getAttribute("aria-checked")).toBe("true");
  await act(async () => button("Read my memory").click());
  await act(async () => button("Read files").click());
  await act(async () => button("Apply").click());
  expect(conciergeClient.setPermission).toHaveBeenCalledExactlyOnceWith("pair", expect.objectContaining({ allow_memory: false, allow_files: false }));
});

it("sets machine access: the folder with its honest note, and a warning for full access", async () => {
  const conciergeClient = concierge([]);
  const runtimeClient = runtime();
  vi.mocked(runtimeClient.permission).mockResolvedValue({ allowed: true, scope: "Project questions", allow_tab_requests: false, machine_access: "folder", folder: "/work/shared",
    effective_folder: "/work/shared", folder_enforcement: { writes: "advisory", reads: "not_confined", note: "Writes are advisory with this agent; reads are not confined." } });
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={runtimeClient} conciergeClient={conciergeClient} />));
  await act(async () => button("Ada, connected").click());
  expect(document.body.textContent).toContain("/work/shared");
  expect(document.body.textContent).toContain("Writes are advisory with this agent; reads are not confined.");
  await act(async () => button("Full access").click());
  expect(document.body.textContent).toContain("Their requests can change anything your agents on this machine can reach.");
  await act(async () => button("Apply").click());
  expect(conciergeClient.setPermission).toHaveBeenCalledExactlyOnceWith("pair", expect.objectContaining({ machine_access: "full" }));
});

it("shows concise read-only limits and keeps the runtime's full note under Details", async () => {
  const runtimeClient = runtime();
  vi.mocked(runtimeClient.permission).mockResolvedValue({ allowed: true, scope: "Questions", allow_tab_requests: false, machine_access: "read_only",
    folder_enforcement: { writes: "pad_only", reads: "not_confined", note: "Writes only its own pad; reads are not confined." } });
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={runtimeClient} conciergeClient={concierge([])} />));
  await act(async () => button("Ada, connected").click());
  expect(document.body.textContent).toContain("Only its own notes folder is writable. Reads are not restricted.");
  const details = Array.from(document.querySelectorAll("details")).find(element => element.textContent?.includes("Writes only its own pad; reads are not confined."));
  expect(details?.querySelector("summary")?.textContent).toBe("Details");
  expect(details?.open).toBe(false);
  expect(details?.querySelector("p")?.textContent).toBe("Writes only its own pad; reads are not confined.");
  expect(document.body.textContent).not.toContain("changes nothing on this machine");
});

it("re-syncs untouched limits to saved values that change after opening, so a scope-only Apply sends only the scope", async () => {
  const filesClient = { send: vi.fn(), list: vi.fn(async () => ({ folder: "/neighbors/ada", files: [] })), open: vi.fn(), openFolder: vi.fn(async () => {}),
    setDailyLimit: vi.fn(async () => {}), usage: vi.fn(async () => null), onFilesChanged: vi.fn(async () => () => {}) };
  const conciergeClient = concierge([]);
  // The window opens on one set of saved limits; the refreshed friend list then reports others (the default file limit as null).
  const before = { ...neighbor, my_daily_limit: 30, my_file_daily_limit_bytes: 52428800, inbound_per_hour: 10 };
  const after = { ...neighbor, my_daily_limit: null, my_file_daily_limit_bytes: null, inbound_per_hour: 15 };
  let friends: NeighborList["friends"] = [before];
  const client: NeighborClient = { list: vi.fn(async (): Promise<NeighborList> => ({ schema_version: 1, access: "paid", can_invite: true, friends, incoming: [], outgoing: [] })),
    apply: vi.fn(async () => {}), claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={runtime()} conciergeClient={conciergeClient} filesClient={filesClient} />));
  await act(async () => button("Ada, connected").click());
  friends = [after];
  await act(async () => button("Refresh Neighbors").click());
  expect(button("Apply").disabled, "saved values changing under untouched fields stage nothing").toBe(true);
  await act(async () => button("Allow requests from Ada").click());
  const scope = document.querySelector<HTMLTextAreaElement>(".neighbor-permissions textarea")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(scope, "Our shared project");
    scope.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => button("Apply").click());
  expect(conciergeClient.setPermission).toHaveBeenCalledOnce();
  expect(conciergeClient.setDailyLimit).not.toHaveBeenCalled();
  expect(filesClient.setDailyLimit).not.toHaveBeenCalled();
  expect(client.apply).not.toHaveBeenCalled();
});

it("blocks files from one friend with a zero daily file limit", async () => {
  const filesClient = { send: vi.fn(), list: vi.fn(async () => ({ folder: "/neighbors/ada", files: [] })), open: vi.fn(), openFolder: vi.fn(async () => {}),
    setDailyLimit: vi.fn(async () => {}), usage: vi.fn(async () => null), onFilesChanged: vi.fn(async () => () => {}) };
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={runtime()} conciergeClient={concierge([])} filesClient={filesClient} />));
  await act(async () => button("Ada, connected").click());
  const input = Array.from(document.querySelectorAll<HTMLLabelElement>("label")).find(label => label.textContent === "Files per day from this friend")!.control as HTMLInputElement;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "0");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => button("Apply").click());
  expect(filesClient.setDailyLimit).toHaveBeenCalledExactlyOnceWith("pair", 0);
});

it("narrows a friend's memory to chosen areas", async () => {
  vi.mocked(localFetch).mockImplementation(async (path: string) => path === "/api/mem/taxonomy"
    ? new Response(JSON.stringify({ work: 3, family: 2, "(none)": 1 }), { status: 200 }) : new Response("null", { status: 404 }));
  const conciergeClient = concierge([]);
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={runtime()} conciergeClient={conciergeClient} />));
  await act(async () => button("Ada, connected").click());
  await act(async () => { button("Memory areas: All areas").dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 })); });
  const options = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitemcheckbox"]'));
  expect(options.map(option => option.textContent)).toEqual(["family", "work"]);
  await act(async () => options.find(option => option.textContent === "work")!.click());
  await act(async () => button("Apply").click());
  expect(conciergeClient.setPermission).toHaveBeenCalledExactlyOnceWith("pair", expect.objectContaining({ memory_areas: ["work"] }));
});

it("fills permissions from a trust preset, turns Custom on any later change, and holds workspaces back", async () => {
  const conciergeClient = concierge([]);
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={runtime()} conciergeClient={conciergeClient} />));
  await act(async () => button("Ada, connected").click());
  expect(button("Custom").getAttribute("aria-selected")).toBe("true");
  await act(async () => button("Acquaintance").click());
  expect(button("Read my memory").getAttribute("aria-checked")).toBe("false");
  expect(button("Acquaintance").getAttribute("aria-selected")).toBe("true");
  // The daemon does not accept workspaces yet, so the editor offers none.
  expect(hasButton("Workspaces: None")).toBe(false);
  await act(async () => button("Apply").click());
  expect(conciergeClient.setPermission).toHaveBeenCalledExactlyOnceWith("pair", expect.objectContaining({ allow_memory: false, allow_files: true }));
  expect(conciergeClient.setDailyLimit).toHaveBeenCalledExactlyOnceWith("pair", 20);
  await act(async () => button("Read files").click());
  expect(button("Custom").getAttribute("aria-selected")).toBe("true");
});

it("shows today's numbers under the friend's name and leaves out what is unknown", async () => {
  const conciergeClient = concierge([{ direction: "in", kind: "question", author: "friend", body: "Hi", at: Date.now(), message_id: "t1" }]);
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, incoming: [], outgoing: [],
    friends: [{ ...neighbor, sent_today: 3, their_daily_limit: 20 }] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={runtime()} conciergeClient={conciergeClient} />));
  await act(async () => button("Ada, connected").click());
  const stats = document.querySelector('[aria-label="Today with this friend"]')?.textContent ?? "";
  expect(stats).toContain("3 of 20 sent today");
  expect(stats).toContain("1 received today");
  expect(stats).not.toContain("files today");
});

it("opens a friend as a window with its two header actions and closes it with Escape", async () => {
  await render();
  await act(async () => button("Ada, connected").click());
  const dialog = document.querySelector<HTMLElement>('[role="dialog"][aria-modal="true"]')!;
  expect(dialog.textContent).toContain("Ada");
  expect(hasButton("Open conversation")).toBe(true);
  expect(hasButton("Talk about Ada")).toBe(true);
  expect(hasButton("Messages")).toBe(false);
  await act(async () => { dialog.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); });
  expect(document.querySelector('[role="dialog"][aria-modal="true"]')).toBeNull();
});

it("saves a cleared daily message limit as no limit", async () => {
  const conciergeClient = concierge([]);
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [{ ...neighbor, my_daily_limit: 5 }], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={runtime()} conciergeClient={conciergeClient} />));
  await act(async () => button("Ada, connected").click());
  const input = Array.from(document.querySelectorAll<HTMLLabelElement>("label")).find(label => label.textContent === "Messages per day")!.control as HTMLInputElement;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(button("Apply").disabled).toBe(false);
  await act(async () => button("Apply").click());
  expect(conciergeClient.setDailyLimit).toHaveBeenCalledExactlyOnceWith("pair", null);
});

it("grows the scope field by its corner grip, height only, and remembers it for this friend", async () => {
  await render();
  await act(async () => button("Ada, connected").click());
  const scope = document.querySelector<HTMLTextAreaElement>(".neighbor-permissions textarea")!;
  const grip = button("Resize what they may ask; use arrow keys");
  await act(async () => { grip.dispatchEvent(new KeyboardEvent("keydown", { code: "ArrowDown", bubbles: true })); });
  await act(async () => { grip.dispatchEvent(new KeyboardEvent("keydown", { code: "ArrowDown", bubbles: true })); });
  expect(scope.style.height).toBe("136px");
  expect(scope.style.width).toBe("");
  expect(localStorage.getItem("domios.neighbors.scope-height:pair")).toBe("136");
});

it("stages edits until Apply, and Cancel puts every field back", async () => {
  const conciergeClient = concierge([]);
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [{ ...neighbor, my_daily_limit: 5 }], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={runtime()} conciergeClient={conciergeClient} />));
  await act(async () => button("Ada, connected").click());
  expect(button("Apply").disabled).toBe(true);
  await act(async () => button("Read files").click());
  const daily = Array.from(document.querySelectorAll<HTMLLabelElement>("label")).find(label => label.textContent === "Messages per day")!.control as HTMLInputElement;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(daily, "9");
    daily.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => button("Cancel").click());
  expect(button("Read files").getAttribute("aria-checked")).toBe("true");
  expect(daily.value).toBe("5");
  expect(button("Apply").disabled).toBe(true);
  expect(conciergeClient.setPermission).not.toHaveBeenCalled();
  expect(conciergeClient.setDailyLimit).not.toHaveBeenCalled();
});

it("opens the friend window at Permissions when another surface asks for it", async () => {
  await render();
  await act(async () => openNeighborPage("pair", "permissions"));
  expect(document.querySelector('[role="dialog"][aria-modal="true"]')?.textContent).toContain("Ada");
  expect(hasButton("Apply")).toBe(true);
});

it("still opens the friend window when the Neighborhood mounts before the account resolves", async () => {
  openNeighborPage("pair");
  account.model = { kind: "pending" };
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => structuredClone(list)), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={runtime()} />));
  account.model = signedIn;
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={runtime()} />));
  expect(document.querySelector('[role="dialog"][aria-modal="true"]')?.textContent).toContain("Ada");
});

it("keeps focus inside the friend window and hands it back to the friend's node on close", async () => {
  await render();
  await act(async () => button("Ada, connected").click());
  const dialog = document.querySelector<HTMLElement>('[role="dialog"][aria-modal="true"]')!;
  const focusable = Array.from(dialog.querySelectorAll<HTMLElement>('button:not([disabled]), [href], input:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'));
  expect(dialog.contains(document.activeElement)).toBe(true);
  focusable[0].focus();
  await act(async () => { dialog.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", shiftKey: true, bubbles: true })); });
  expect(document.activeElement).toBe(focusable[focusable.length - 1]);
  await act(async () => { dialog.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); });
  expect(document.activeElement?.getAttribute("data-node-id")).toBe("pair");
});

it("shows today's received count as a floor while older pages may hold more of today", async () => {
  const now = Date.now();
  const entries: HistoryEntry[] = [
    { direction: "in", kind: "question", author: "friend", body: "First", at: now - 2, message_id: "m1" },
    { direction: "in", kind: "question", author: "friend", body: "Second", at: now - 1, message_id: "m2" },
  ];
  const conciergeClient = concierge(entries);
  vi.mocked(conciergeClient.history).mockResolvedValue({ entries, next_before: "m1", ended: false });
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={runtime()} conciergeClient={conciergeClient} />));
  await act(async () => button("Ada, connected").click());
  expect(document.querySelector('[aria-label="Today with this friend"]')?.textContent).toContain("2+ received today");
});

it("disables a friend from the window header's Enabled switch and enables them again", async () => {
  const conciergeClient = concierge([]);
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={runtime()} conciergeClient={conciergeClient} />));
  await act(async () => button("Ada, connected").click());
  expect(button("Enabled for Ada").getAttribute("aria-checked")).toBe("true");
  await act(async () => button("Enabled for Ada").click());
  expect(conciergeClient.setPaused).toHaveBeenLastCalledWith("pair", true);
  expect(button("Enabled for Ada").getAttribute("aria-checked")).toBe("false");
  expect(document.body.textContent).toContain("You disabled Ada. They can't write to you until you enable them again");
  await act(async () => button("Enabled for Ada").click());
  expect(conciergeClient.setPaused).toHaveBeenLastCalledWith("pair", false);
});

it("shows a friend you disabled as disabled on the graph", async () => {
  await render("paid", { friend: { ...neighbor, my_paused: true } });
  expect(hasButton("Ada, disabled")).toBe(true);
  expect(document.querySelector('[data-node-id="pair"]')?.hasAttribute("data-paused")).toBe(true);
});

it("defaults write areas to empty and saves only selected known memory areas", async () => {
  vi.mocked(localFetch).mockImplementation(async (path: string) => path === "/api/mem/taxonomy"
    ? new Response(JSON.stringify({ work: 3, family: 2, "(none)": 1 }), { status: 200 }) : new Response("null", { status: 404 }));
  const conciergeClient = concierge([]);
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={runtime()} conciergeClient={conciergeClient} />));
  await act(async () => button("Ada, connected").click());
  await act(async () => { button("Memory write areas: None").dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 })); });
  const options = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitemcheckbox"]'));
  expect(options.map(option => option.textContent)).toEqual(["family", "work"]);
  await act(async () => options.find(option => option.textContent === "work")!.click());
  await act(async () => button("Apply").click());
  expect(conciergeClient.setPermission).toHaveBeenCalledExactlyOnceWith("pair", expect.objectContaining({ memory_write_areas: ["work"] }));
});

it("shows friend email in the panel and tooltip and incoming request identity", async () => {
  const { list } = await render("paid", { friend: { ...neighbor, email: "ada@example.test" } });
  expect(button("Ada, connected").title).toBe("Ada (ada@example.test)");
  await act(async () => button("Ada, connected").click());
  expect(document.querySelector('[role="dialog"]')?.textContent).toContain("ada@example.test");
  await act(async () => button("Close Ada").click());
  const next = { ...list, incoming: list.incoming.map(request => ({ ...request, email: "grace@example.test" })) };
  await act(async () => emit("neighbors-changed", { uid: "owner", list: next }));
  await act(async () => button("Grace, wants to connect").click());
  expect(document.querySelector("aside")?.textContent).toContain("Grace (grace@example.test) wants to be your Neighbor");
});

it("uses email before uid when a friend has no display name", async () => {
  await render("paid", { friend: { ...neighbor, displayName: null, email: "ada@example.test" } });
  expect(hasButton("ada@example.test, connected")).toBe(true);
  expect(document.body.textContent).not.toContain(neighbor.uid);
});
