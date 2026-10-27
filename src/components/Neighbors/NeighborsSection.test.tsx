import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { NeighborsInventory } from "./NeighborsSection";
import type { NeighborClient } from "./neighborClient";
import type { NeighborList } from "./neighborModel";
import { parseNeighbors } from "./neighborModel";
import { parseReferralClaim, type ReferralClaim } from "./referralTrial";
import { neighborClient } from "./neighborClient";
import { localFetch } from "../../lib/webApi";

const notices = vi.hoisted(() => ({ publishNotice: vi.fn() }));
const transport = vi.hoisted(() => ({ localFetch: vi.fn() }));
const events = vi.hoisted(() => ({ changed: null as null | (() => void) }));
vi.mock("../../lib/webApi", async actual => ({ ...await actual<object>(), ...transport }));
vi.mock("../../lib/notices", () => notices);
vi.mock("../../lib/bridge", () => ({ listen: vi.fn(async (_event: string, handler: () => void) => {
  events.changed = handler;
  return () => { events.changed = null; };
}) }));
vi.mock("../Settings/useAccount", () => ({ useAccount: () => ({ model: { kind: "signed_out" } }) }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const neighbor = { uid: "peer", displayName: "Ada", photoURL: null, friendship_id: "pair", inbound_per_hour: 10, outbound_per_hour: 20, since: 1700000000 };
const request = { uid: "requester", displayName: "Grace", photoURL: null, request_id: "request", created_at: 1700000000 };
let mounted: { root: ReturnType<typeof createRoot>; host: HTMLDivElement } | null = null;
afterEach(async () => {
  if (mounted) { await act(async () => mounted!.root.unmount()); mounted.host.remove(); mounted = null; }
  localStorage.clear();
  notices.publishNotice.mockClear();
  transport.localFetch.mockReset();
});

function button(label: string): HTMLButtonElement {
  const found = Array.from(document.querySelectorAll("button")).find(element => element.textContent === label);
  if (!found) throw new Error(`Missing button ${label}`);
  return found;
}

async function render(access: NeighborList["access"] = "paid", options: { canInvite?: boolean; omitEligibility?: boolean; claim?: ReferralClaim } = {}) {
  const list: NeighborList = { schema_version: 1, access, can_invite: options.omitEligibility ? undefined : options.canInvite ?? access === "paid", friends: [neighbor], incoming: [request] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => options.claim ?? { schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null }) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted = { root, host };
  const onOpenGeneral = vi.fn();
  await act(async () => root.render(<NeighborsInventory identity="owner" client={client} onOpenGeneral={onOpenGeneral} />));
  await act(async () => button("Neighbors").click());
  return { client, list, onOpenGeneral };
}

it("opens and collapses the registry and notifies once for a new request", async () => {
  const { client } = await render();
  expect(button("Neighbors").getAttribute("aria-expanded")).toBe("true");
  expect(notices.publishNotice).toHaveBeenCalledTimes(1);
  await act(async () => button("Refresh Neighbors").click());
  expect(client.list).toHaveBeenCalledTimes(2);
  expect(notices.publishNotice).toHaveBeenCalledTimes(1);
  await act(async () => button("Neighbors").click());
  expect(button("Neighbors").getAttribute("aria-expanded")).toBe("false");
});

it("saves an email request once and never claims email delivery", async () => {
  const { client } = await render();
  await act(async () => button("Add Neighbor").click());
  const input = document.querySelector<HTMLInputElement>('input[type="email"]')!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "friend@example.test");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => document.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })));
  expect(client.apply).toHaveBeenCalledExactlyOnceWith({ operation: "request", email: "friend@example.test" });
  expect(button("Send request or invitation").disabled).toBe(true);
});

it("lets Free accept and manage dormant friendships while initiation offers plans", async () => {
  const { client } = await render("locked");
  expect(button("Upgrade to add Neighbors")).toBeDefined();
  await act(async () => button("Accept Grace").click());
  expect(client.apply).toHaveBeenCalledWith({ operation: "respond", request_id: "request", accept: true });
  await act(async () => button("Ada").click());
  const input = document.querySelector<HTMLInputElement>('input[type="number"]')!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "42");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => button("Save limit").click());
  expect(client.apply).toHaveBeenCalledWith({ operation: "set_limit", friendship_id: "pair", per_hour: 42 });
  await act(async () => button("Remove Neighbor").click());
  expect(client.apply).not.toHaveBeenCalledWith({ operation: "remove", friendship_id: "pair" });
  await act(async () => button("Confirm remove").click());
  expect(client.apply).toHaveBeenCalledWith({ operation: "remove", friendship_id: "pair" });
});

it("links the permissions hint to the General Agent", async () => {
  const { onOpenGeneral } = await render();
  await act(async () => button("Ada").click());
  await act(async () => button("Open General Agent").click());
  expect(onOpenGeneral).toHaveBeenCalledTimes(1);
  expect(document.querySelector('[role="dialog"]')).toBeNull();
});

it.each([false, undefined])("requires explicit server invitation permission, even with paid channel access (%s)", async canInvite => {
  const { client } = await render("paid", { canInvite, omitEligibility: canInvite === undefined });
  expect(Array.from(document.querySelectorAll("button")).some(element => element.textContent === "Add Neighbor")).toBe(false);
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
  expect(document.querySelector('input[type="email"]')).toBeNull();
  expect(client.apply).not.toHaveBeenCalled();
  await act(async () => button("Refresh Neighbors").click());
  expect(client.claimReferralTrial).toHaveBeenCalledTimes(2);
  expect(client.apply).not.toHaveBeenCalled();
});

it("keeps a server-locked expired referral manageable and offers subscription", async () => {
  const { client } = await render("locked", { canInvite: false, claim: {
    schema_version: 1, outcome: "already_granted", trial_end: 1700604800, friend_request_id: null,
  } });
  expect(button("Subscribe")).toBeDefined();
  await act(async () => button("Ada").click());
  expect(button("View plans")).toBeDefined();
  expect(Array.from(document.querySelectorAll("button")).some(element => element.textContent === "Enable encrypted channel")).toBe(false);
  expect(client.apply).not.toHaveBeenCalled();
});

it("preserves an already-paid recipient's connection controls without a referral checkout", async () => {
  const { client } = await render("paid", { claim: { schema_version: 1, outcome: "already_paid", trial_end: null, friend_request_id: null } });
  expect(Array.from(document.querySelectorAll("button")).some(element => element.textContent === "Subscribe")).toBe(false);
  await act(async () => button("Accept Grace").click());
  expect(client.apply).toHaveBeenCalledExactlyOnceWith({ operation: "respond", request_id: "request", accept: true });
});

it("discards a claim that completes after switching accounts", async () => {
  let complete!: (claim: ReferralClaim) => void;
  const previous = new Promise<ReferralClaim>(resolve => { complete = resolve; });
  const client: NeighborClient = {
    claimReferralTrial: vi.fn<NeighborClient["claimReferralTrial"]>().mockReturnValueOnce(previous).mockResolvedValue({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null }),
    list: vi.fn(async () => ({ schema_version: 1 as const, access: "locked" as const, can_invite: false, friends: [], incoming: [] })),
    apply: vi.fn(async () => {}),
  };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted = { root, host };
  await act(async () => root.render(<NeighborsInventory identity="previous" client={client} onOpenGeneral={vi.fn()} />));
  await act(async () => root.render(<NeighborsInventory identity="current" client={client} onOpenGeneral={vi.fn()} />));
  await act(async () => complete({ schema_version: 1, outcome: "granted", trial_end: 1700604800, friend_request_id: "a".repeat(64) }));
  expect(client.list).toHaveBeenCalledTimes(1);
  expect(Array.from(document.querySelectorAll("button")).some(element => element.textContent === "Subscribe")).toBe(false);
});

it("rejects unrecognized or malformed claims and never substitutes channel access for invite permission", () => {
  for (const value of [
    { schema_version: 2, outcome: "granted", trial_end: 1700604800, friend_request_id: null },
    { schema_version: 1, outcome: "granted", trial_end: null, friend_request_id: null },
    { schema_version: 1, outcome: "not_invited", trial_end: 1700604800, friend_request_id: null },
    { schema_version: 1, outcome: "unknown", trial_end: null, friend_request_id: null },
    { schema_version: 1, outcome: "granted", trial_end: 1700604800, friend_request_id: "foreign" },
  ]) expect(() => parseReferralClaim(value)).toThrow();
  expect(parseNeighbors({ schema_version: 1, access: "paid", friends: [], incoming: [] }).can_invite).toBeUndefined();
  expect(() => parseNeighbors({ schema_version: 1, access: "paid", can_invite: "yes", friends: [], incoming: [] })).toThrow();
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

it("links a signed-out invitee to account creation without making an invitation or checkout", async () => {
  const onPanel = vi.fn();
  window.addEventListener("codeterm:open-panel", onPanel);
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted = { root, host };
  try {
    await act(async () => root.render(<NeighborsInventory identity={null} onOpenGeneral={vi.fn()} />));
    await act(async () => button("Neighbors").click());
    await act(async () => button("Sign in or create account").click());
    expect(onPanel).toHaveBeenCalledTimes(1);
    expect((onPanel.mock.calls[0][0] as CustomEvent).detail).toEqual({ panel: "settings:account" });
    expect(localFetch).not.toHaveBeenCalled();
  } finally { window.removeEventListener("codeterm:open-panel", onPanel); }
});

it("does not unlock a server-locked recipient even when the claim reports a future trial end", async () => {
  const { client } = await render("locked", { canInvite: false, claim: {
    schema_version: 1, outcome: "granted", trial_end: 4102444800, friend_request_id: null,
  } });
  await act(async () => button("Ada").click());
  expect(button("View plans")).toBeDefined();
  expect(Array.from(document.querySelectorAll("button")).some(element => element.textContent === "Enable encrypted channel")).toBe(false);
  expect(client.apply).not.toHaveBeenCalled();
});

it("refreshes the server's channel lock when signed entitlement access changes", async () => {
  const { list } = await render("paid", { canInvite: false, claim: {
    schema_version: 1, outcome: "already_granted", trial_end: 1700604800, friend_request_id: null,
  } });
  await act(async () => button("Ada").click());
  expect(Array.from(document.querySelectorAll("button")).some(element => element.textContent === "View plans")).toBe(false);
  list.access = "locked";
  expect(events.changed).not.toBeNull();
  await act(async () => events.changed!());
  expect(button("View plans")).toBeDefined();
  expect(button("Subscribe")).toBeDefined();
});
