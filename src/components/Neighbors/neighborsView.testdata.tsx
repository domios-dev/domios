import { createRoot } from "react-dom/client";
import { afterEach, vi } from "vitest";
import { act } from "react";
import { resetPausedFriends } from "./neighborPause";
import type { NeighborRuntimeClient } from "./neighborRuntimeClient";
import type { NeighborList } from "./neighborModel";
import type { ReferralClaim } from "./referralTrial";
import type { NeighborClient } from "./neighborClient";
import { NeighborsView } from "./NeighborsView";
import { SidebarNeighborsSection } from "./SidebarNeighborsSection";
import type { HistoryEntry, NeighborConciergeClient, NeighborNotify } from "./neighborConciergeClient";
import { NeighborConversationView } from "./NeighborConversationView";
import type { createNeighborMocks } from "./neighborsMocks.testdata";

export function createNeighborsViewFixture({ notices, transport, bridge, account }: ReturnType<typeof createNeighborMocks>) {
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const neighbor = { uid: "peer", displayName: "Ada", photoURL: null, friendship_id: "pair", inbound_per_hour: 10, outbound_per_hour: 20, receiving: true, since: 1700000000 };
const request = { uid: "requester", displayName: "Grace", photoURL: null, request_id: "request", created_at: 1700000000 };
const signedIn = account.model;
const mounted: { current: { root: ReturnType<typeof createRoot>; host: HTMLDivElement } | null } = { current: null };
afterEach(async () => {
  if (mounted.current) { await act(async () => mounted.current!.root.unmount()); mounted.current.host.remove(); mounted.current = null; }
  localStorage.clear();
  resetPausedFriends();
  notices.publishNotice.mockClear();
  transport.localFetch.mockReset();
  transport.localFetch.mockImplementation(async () => new Response("{}", { status: 200 }));
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
    subscribe: vi.fn(async () => () => {}), status: vi.fn(async () => ({ enrolled: false, bindings, receiving_here: false, receiving_enabled: false, receiver_machine_label: null, revision: 0 })), enroll: vi.fn(async () => {}), receiveFriend: vi.fn(async () => {}), rotate: vi.fn(async () => {}),
    permission: vi.fn(async () => ({ allowed: false, scope: "", allow_tab_requests: false })), send: vi.fn(), deliveryStatus: vi.fn(), retry: vi.fn(async () => {}),
  };
}

async function render(access: NeighborList["access"] = "paid", options: { canInvite?: boolean; omitEligibility?: boolean; claim?: ReferralClaim; bindings?: Record<string, string>; outgoing?: NeighborList["outgoing"]; runtimeClient?: NeighborRuntimeClient; friend?: NeighborList["friends"][number] } = {}) {
  const list: NeighborList = { schema_version: 1, access, can_invite: options.omitEligibility ? undefined : options.canInvite ?? access === "paid", friends: [options.friend ?? neighbor], incoming: [request], outgoing: options.outgoing ?? [] };
  const client: NeighborClient = { list: vi.fn(async () => structuredClone(list)), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => options.claim ?? { schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null }) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborsView client={client} runtimeClient={options.runtimeClient ?? runtime(options.bindings)} />));
  return { client, list };
}

const sentText = () => (bridge.invoke.mock.calls as unknown as Array<[string, { tabId: string; text: string }]>)
  .filter(([command]) => command === "chat_send_text").map(([, args]) => args);

async function renderSection(friends: NeighborList["friends"], bindings: Record<string, string> = {}) {
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends, incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<SidebarNeighborsSection touchMode={false} client={client} runtimeClient={runtime(bindings)} />));
}

function concierge(entries: HistoryEntry[]): NeighborConciergeClient & { changed: (friendship: string) => void } {
  let changed = (_friendship: string) => {};
  return {
    history: vi.fn(async () => ({ entries: [...entries].reverse(), next_before: null, ended: false })),
    onHistoryChanged: vi.fn(async (handler: (friendship: string) => void) => { changed = handler; return () => {}; }),
    onPermissionChanged: vi.fn(async () => () => {}), onAgentChanged: vi.fn(async () => () => {}),
    setPermission: vi.fn(async () => {}), changeAgent: vi.fn(async () => "tab-gateway"),
    setDailyLimit: vi.fn(async () => {}), onDailyLimitChanged: vi.fn(async () => () => {}),
    setNotify: vi.fn(async (_friendship: string, notify: NeighborNotify) => notify), onNotifyChanged: vi.fn(async () => () => {}),
    setPaused: vi.fn(async (_friendship: string, paused: boolean) => paused), onPausedChanged: vi.fn(async () => () => {}),
    stopConcierge: vi.fn(async () => {}),
    changed: (friendship: string) => changed(friendship),
  };
}

function pendingClient(): NeighborClient {
  return { list: vi.fn(() => new Promise<NeighborList>(() => {})), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
}

/** Ada as the one paid friend, so a conversation renders at once. */
function friendClient(): NeighborClient {
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  return { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
}

async function renderConversation(friend: NeighborList["friends"][number]) {
  const conciergeClient = concierge([]);
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [friend], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}),
    claimReferralTrial: vi.fn(async (): Promise<ReferralClaim> => ({ schema_version: 1, outcome: "not_invited", trial_end: null, friend_request_id: null })) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={client} runtimeClient={runtime()} conciergeClient={conciergeClient} />));
  return conciergeClient;
}
return { neighbor, request, signedIn, mounted, button, hasButton, emit, runtime, render, sentText, renderSection, concierge, pendingClient, friendClient, renderConversation };
}
