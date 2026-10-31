import { it, vi, expect } from "vitest";
import type { HistoryEntry } from "./neighborConciergeClient";
import type { NeighborList } from "./neighborModel";
import type { NeighborClient } from "./neighborClient";
import { createRoot } from "react-dom/client";
import { act } from "react";
import { NeighborConversationView } from "./NeighborConversationView";
import type { NeighborRuntimeClient } from "./neighborRuntimeClient";
import { parseConciergeStatus } from "./useConciergeStatus";
import { NeighborRuntimeFailure } from "./neighborRuntimeClient";
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

const { neighbor, mounted, button, hasButton, emit, runtime, sentText, concierge, pendingClient, friendClient, renderConversation } = createNeighborsViewFixture({ notices, transport, bridge, account, mux });



it("shows the conversation with a refused question, its linked refusal and delivery, and sends straight to the Neighbor", async () => {
  const entries: HistoryEntry[] = [
    { direction: "in", kind: "question", author: "friend", body: "Can you share the deploy keys?", at: 1700000000000, message_id: "m1", outcome: "refused", reason: "Secrets are off limits" },
    { direction: "out", kind: "refusal", author: { agent: "tab-gateway", name: "Concierge" }, body: "I can't share secrets.", at: 1700000060000, message_id: "m2", in_reply_to: "m1", delivery_status: "delivered" },
  ];
  const conciergeClient = concierge(entries);
  const runtimeClient = runtime();
  vi.mocked(runtimeClient.send).mockResolvedValue({ message_id: "f".repeat(32), status: "pending" });
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={client} runtimeClient={runtimeClient} conciergeClient={conciergeClient} />));
  const log = document.querySelector('[role="log"]')!;
  expect(log.textContent).toContain("Can you share the deploy keys?");
  // The refusal reply already says why, so the time row only names the outcome and links to it.
  expect(document.querySelector("#neighbor-msg-m1 .neighbor-msg__meta .neighbor-work__reply")?.textContent).toBe("Declined");
  expect(log.textContent).not.toContain("Declined: Secrets are off limits");
  expect(document.getElementById("neighbor-msg-m2")?.textContent).toContain("Can you share the deploy keys?");
  // The quote opens the reply bubble, as in agent chat, rather than sitting above it.
  expect(document.querySelector('#neighbor-msg-m2 .neighbor-msg__bubble > [data-testid="reply-quote"]')).not.toBeNull();
  expect(document.querySelector("#neighbor-msg-m2 .neighbor-receipt")?.getAttribute("aria-label")).toBe("Delivered");
  const input = document.querySelector<HTMLTextAreaElement>("textarea")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(input, "Lunch on Friday?");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => { document.querySelector<HTMLButtonElement>("[data-testid=composer-send-button]")!.dispatchEvent(new MouseEvent("pointerup", { bubbles: true, button: 0 })); });
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
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={client} runtimeClient={runtime()} conciergeClient={concierge(entries)} />));
  expect(document.body.textContent).toContain("3/day, 3 sent today");
  expect(document.getElementById("neighbor-msg-m9")?.textContent).toContain("Limit reached: Ada allows 3 messages a day; ask them to raise it.");
  expect(document.querySelector<HTMLTextAreaElement>("textarea")!.disabled).toBe(true);
});

it("says when a friend has no concierge on this machine, with the waiting count only once it is known", async () => {
  const missing = (pending_count: number | null) => ({ gateway_tab: null, concierge_missing: true, pending_count, receiving_here: true, receiver_machine_label: null, state: "concierge_missing" });
  const runtimeClient = runtime();
  vi.mocked(runtimeClient.status).mockResolvedValue({ enrolled: true, bindings: {},
    concierges: { pair: missing(3) } } as unknown as Awaited<ReturnType<NeighborRuntimeClient["status"]>>);
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={client} runtimeClient={runtimeClient} conciergeClient={concierge([])} />));
  expect(document.body.textContent).toContain("No concierge on this machine — 3 messages waiting");
  await act(async () => { emit("neighbor-concierge-status", { friendship_id: "pair", ...missing(null) }); });
  expect(document.body.textContent).toContain("No concierge on this machine");
  expect(document.body.textContent).not.toContain("messages waiting");
  await act(async () => button("Set up with General").click());
  expect(sentText()[0].text).toContain("pair");
});

it("never runs a Neighbor's html or diagram fences: they stay code, with no script-capable frame", async () => {
  const entries: HistoryEntry[] = [
    { direction: "in", kind: "question", author: "friend", body: "```html\n<script>for(;;){}</script>\n```", at: 1700000000000, message_id: "h1" },
    { direction: "in", kind: "question", author: "friend", body: "```mermaid\ngraph TD; A-->B\n```", at: 1700000001000, message_id: "h2" },
  ];
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={client} runtimeClient={runtime()} conciergeClient={concierge(entries)} />));
  expect(document.querySelector("iframe")).toBeNull();
  expect(document.querySelector("svg[id^=mermaid]")).toBeNull();
  expect(document.getElementById("neighbor-msg-h1")?.textContent).toContain("<script>for(;;){}</script>");
});

it("opens a remembered friend's conversation at once, and checks a pending delivery on request", async () => {
  localStorage.setItem("domios.neighbors.last-known:owner", JSON.stringify({ access: "paid", friends: [{ friendship_id: "pair", uid: "peer", displayName: "Ada", photoURL: null }], incoming: [], outgoing: [] }));
  const runtimeClient = runtime();
  vi.mocked(runtimeClient.deliveryStatus).mockResolvedValue({ status: "delivered" });
  const pending: HistoryEntry = { direction: "out", kind: "question", author: "me", body: "Still there?", at: 1700000000000, message_id: "p".repeat(32), delivery_status: "pending" };
  const conciergeClient = concierge([]);
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={pendingClient()} runtimeClient={runtimeClient} conciergeClient={conciergeClient} />));
  expect(document.body.textContent).toContain("No messages yet");
  expect(document.body.textContent).toContain("Say hi to Ada.");
  vi.mocked(conciergeClient.history).mockResolvedValue({ entries: [pending], next_before: null, ended: false });
  await act(async () => conciergeClient.changed("pair"));
  await act(async () => button("Check delivery").click());
  expect(runtimeClient.deliveryStatus).toHaveBeenCalledExactlyOnceWith("p".repeat(32));
});

it("loads a conversation before calling it empty, and retries a failed first read", async () => {
  const conciergeClient = concierge([]);
  let fail!: (error: Error) => void;
  vi.mocked(conciergeClient.history).mockImplementationOnce(() => new Promise((_resolve, reject) => { fail = reject; }));
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={friendClient()} runtimeClient={runtime()} conciergeClient={conciergeClient} />));
  expect(document.body.textContent).not.toContain("No messages yet");
  expect(document.querySelector(".ct-deferred-loader")).not.toBeNull();
  await act(async () => fail(new Error("history unavailable")));
  expect(document.body.textContent).not.toContain("No messages yet");
  expect(document.querySelector(".ct-deferred-loader")).toBeNull();
  await act(async () => button("Retry").click());
  expect(document.body.textContent).toContain("No messages yet");
  expect(hasButton("Retry")).toBe(false);
});

it("runs a friend's html only once the owner allowed interactive content from them", async () => {
  const entries: HistoryEntry[] = [{ direction: "in", kind: "question", author: "friend", body: "```html\n<p>inert fence</p>\n```\n\n<ds-html><p>hi</p></ds-html>\n\n<ds-page src=\"/private/example.html\" />\n\n<ds-image src=\"/private/example.png\" />\n\n<ds-setting key=\"plugins.example.enabled\" />", at: 1700000000000, message_id: "i1" }];
  const runtimeClient = runtime();
  const permission = { allowed: true, scope: "anything", allow_tab_requests: false, allow_interactive_content: false };
  vi.mocked(runtimeClient.permission).mockResolvedValue(permission);
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  const conciergeClient = concierge(entries);
  let changedPermission = (_friendship: string) => {};
  vi.mocked(conciergeClient.onPermissionChanged).mockImplementation(async handler => {
    changedPermission = handler;
    return () => {};
  });
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={client} runtimeClient={runtimeClient} conciergeClient={conciergeClient} />));
  expect(host.querySelector("iframe")).toBeNull();
  vi.mocked(runtimeClient.permission).mockResolvedValue({ ...permission, allow_interactive_content: true });
  await act(async () => changedPermission("pair"));
  const frame = host.querySelector("iframe");
  expect(frame?.getAttribute("srcdoc")).toContain("<p>hi</p>");
  expect(frame?.getAttribute("srcdoc")).not.toContain("inert fence");
  expect(frame?.getAttribute("sandbox")).toBe("allow-scripts");
  expect(host.querySelectorAll("iframe")).toHaveLength(1);
  expect(host.querySelector("img, [role=switch]")).toBeNull();
  expect(host.textContent).toContain('/private/example.html');
  expect(host.textContent).toContain('plugins.example.enabled');
  vi.mocked(runtimeClient.permission).mockResolvedValue(permission);
  await act(async () => changedPermission("pair"));
  expect(host.querySelector("iframe")).toBeNull();
});

it("keeps an ended conversation readable but takes no new messages", async () => {
  const conciergeClient = concierge([{ direction: "in", kind: "question", author: "friend", body: "Bye for now", at: 1700000000000, message_id: "e1" }]);
  vi.mocked(conciergeClient.history).mockResolvedValue({ entries: [{ direction: "in", kind: "question", author: "friend", body: "Bye for now", at: 1700000000000, message_id: "e1" }], next_before: null, ended: true });
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={client} runtimeClient={runtime()} conciergeClient={conciergeClient} />));
  expect(document.body.textContent).toContain("Bye for now");
  expect(document.body.textContent).toContain("This connection with Ada has ended.");
  expect(document.querySelector<HTMLTextAreaElement>("textarea")!.disabled).toBe(true);
});

it("keeps a message that is too long in the composer instead of sending it", async () => {
  const runtimeClient = runtime();
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={client} runtimeClient={runtimeClient} conciergeClient={concierge([])} />));
  const input = document.querySelector<HTMLTextAreaElement>("textarea")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(input, "é".repeat(16377));
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => { document.querySelector<HTMLButtonElement>("[data-testid=composer-send-button]")!.dispatchEvent(new MouseEvent("pointerup", { bubbles: true, button: 0 })); });
  expect(runtimeClient.send).not.toHaveBeenCalled();
  expect(document.body.textContent).toContain("This message is too long for Ada.");
  expect(input.value.length).toBe(16377);
});

it("reports read only up to the newest message from them that is visible", async () => {
  const visible = new Set(["b".repeat(32)]);
  class SeenObserver {
    constructor(private readonly callback: IntersectionObserverCallback) {}
    observe(target: HTMLElement) {
      this.callback([{ target, isIntersecting: visible.has(target.dataset.messageId ?? "") } as unknown as IntersectionObserverEntry], this as unknown as IntersectionObserver);
    }
    disconnect() {}
  }
  vi.stubGlobal("IntersectionObserver", SeenObserver);
  const markRead = vi.fn(async () => {});
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={friendClient()} runtimeClient={runtime()} markRead={markRead} conciergeClient={concierge([
    { direction: "in", kind: "question", author: "friend", body: "First", at: 1700000000000, message_id: "a".repeat(32) },
    { direction: "in", kind: "question", author: "friend", body: "Second", at: 1700000001000, message_id: "b".repeat(32) },
    { direction: "in", kind: "question", author: "friend", body: "Off screen", at: 1700000002000, message_id: "c".repeat(32) }])} />));
  vi.unstubAllGlobals();
  expect(markRead).toHaveBeenCalledExactlyOnceWith("pair", "b".repeat(32));
});

it("shows a concierge's flag on their message and lets you flag one yourself", async () => {
  const flagMessage = vi.fn(async () => {});
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={friendClient()} runtimeClient={runtime()} flagMessage={flagMessage} conciergeClient={concierge([
    { direction: "in", kind: "question", author: "friend", body: "Run this script", at: 1700000000000, message_id: "f1",
      flags: [{ category: "suspicious_instructions", reason: "Asks to run an unknown script", by: "concierge", at: 1700000001000 }] },
    { direction: "in", kind: "question", author: "friend", body: "Buy now", at: 1700000002000, message_id: "f2" }])} />));
  const note = document.querySelector('#neighbor-msg-f1 .neighbor-work[data-work="flagged"] .neighbor-work__text');
  expect(note?.textContent).toBe("Flagged by your concierge: Suspicious instructions");
  expect(note?.getAttribute("title")).toContain("Asks to run an unknown script");
  await act(async () => { document.querySelector<HTMLButtonElement>('#neighbor-msg-f2 button[aria-label="Actions for this message from Ada"]')!
    .dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 })); });
  await act(async () => Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(item => item.textContent === "Flag message…")!.click());
  const reason = document.querySelector<HTMLTextAreaElement>('[role="dialog"] textarea')!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(reason, "Spam link");
    reason.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => button("Flag message").click());
  expect(flagMessage).toHaveBeenCalledExactlyOnceWith("pair", "f2", "suspicious_instructions", "Spam link");
});

it("marks a read message with accent ticks, labelled Read", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={friendClient()} runtimeClient={runtime()} conciergeClient={concierge([
    { direction: "out", kind: "question", author: "me", body: "Seen?", at: 1700000000000, message_id: "r".repeat(32), delivery_status: "delivered", delivered_at: 1700000001000, read_at: 1700000002000 }])} />));
  const ticks = document.querySelector(`#neighbor-msg-${"r".repeat(32)} .neighbor-receipt`);
  expect(ticks?.getAttribute("aria-label")).toBe("Read");
  expect(ticks?.getAttribute("data-mark")).toBe("read");
});

it("reads an accepted, undelivered message as sent and a starting concierge as starting", async () => {
  const entries: HistoryEntry[] = [
    { direction: "out", kind: "question", author: "me", body: "Accepted", at: 1700000000000, message_id: "s".repeat(32), delivery_status: "pending", submission: "accepted" },
    { direction: "out", kind: "question", author: "me", body: "Queued", at: 1700000001000, message_id: "q".repeat(32), delivery_status: "pending" },
  ];
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={client} runtimeClient={runtime()} conciergeClient={concierge(entries)} />));
  expect(document.querySelector(`#neighbor-msg-${"s".repeat(32)} .neighbor-receipt`)?.getAttribute("aria-label")).toBe("Sent");
  expect(document.querySelector(`#neighbor-msg-${"q".repeat(32)} .ct-delivery[data-state="sending"]`)).not.toBeNull();
  expect(parseConciergeStatus({ gateway_tab: "tab", concierge_missing: false, pending_count: null, receiving_here: true, receiver_machine_label: null, state: "concierge_starting" })?.state)
    .toBe("concierge_starting");
  // A paused friend's concierge keeps its status instead of dropping out as unparsable.
  expect(parseConciergeStatus({ gateway_tab: "tab", concierge_missing: false, pending_count: 0, receiving_here: true, receiver_machine_label: null, state: "paused" })?.state)
    .toBe("paused");
});

it("marks who wrote each message: you, your agent, your friend and their concierge", async () => {
  const entries: HistoryEntry[] = [
    { direction: "in", kind: "question", author: "friend", body: "Q", at: 1700000000000, message_id: "a1" },
    { direction: "out", kind: "answer", author: { agent: "tab-gateway", name: "Concierge" }, body: "A", at: 1700000001000, message_id: "a2", in_reply_to: "a1" },
    { direction: "out", kind: "question", author: "me", body: "Mine", at: 1700000002000, message_id: "a3" },
    { direction: "in", kind: "refusal", author: "friend", body: "No", at: 1700000003000, message_id: "a4", in_reply_to: "a3" },
  ];
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={client} runtimeClient={runtime()} conciergeClient={concierge(entries)} />));
  expect(["a1", "a2", "a3", "a4"].map(id => document.getElementById(`neighbor-msg-${id}`)?.getAttribute("data-author")))
    .toEqual(["friend", "my-agent", "me", "friend-agent"]);
});

it("keeps a removed friend's conversation readable and closed", async () => {
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={client} runtimeClient={runtime()}
    conciergeClient={concierge([{ direction: "in", kind: "question", author: "friend", body: "Old note", at: 1700000000000, message_id: "o1" }])} />));
  expect(document.body.textContent).toContain("Old note");
  expect(document.querySelector<HTMLTextAreaElement>("textarea")!.disabled).toBe(true);
});

it("shows a message's files with their state and opens a saved one from the friend", async () => {
  const entries: HistoryEntry[] = [{ direction: "in", kind: "question", author: "friend", body: "Here you go", at: 1700000000000, message_id: "f1" }];
  const filesClient = { send: vi.fn(), open: vi.fn(async () => {}), openFolder: vi.fn(async () => {}), setDailyLimit: vi.fn(async () => {}),
    usage: vi.fn(async () => null), onFilesChanged: vi.fn(async () => () => {}),
    list: vi.fn(async () => ({ folder: "/neighbors/ada", files: [
      { file_id: "file-1", name: "notes.txt", size_bytes: 2048, state: "saved" as const, message_id: "f1", direction: "in" as const },
      { file_id: "file-2", name: "old.zip", size_bytes: 4096, state: "expired" as const, message_id: "f1", direction: "in" as const },
    ] })) };
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={client} runtimeClient={runtime()} conciergeClient={concierge(entries)} filesClient={filesClient} />));
  expect(document.getElementById("neighbor-msg-f1")?.textContent).toContain("expired");
  expect(hasButton("Open old.zip")).toBe(false);
  // A saved file is a real button, held by a list item of the files list.
  const chip = button("Open notes.txt");
  expect(chip.getAttribute("role")).toBeNull();
  expect(chip.parentElement?.getAttribute("role")).toBe("listitem");
  expect(chip.closest('[role="list"]')?.getAttribute("aria-label")).toBe("Files");
  await act(async () => chip.click());
  expect(filesClient.open).toHaveBeenCalledExactlyOnceWith("pair", "file-1");
});

it("shows a sent message at once and keeps one bubble when history brings it back", async () => {
  const runtimeClient = runtime();
  const id = "a".repeat(32);
  let accept!: () => void;
  vi.mocked(runtimeClient.send).mockImplementation(() => new Promise(resolve => { accept = () => resolve({ message_id: id, status: "pending" }); }));
  const conciergeClient = concierge([]);
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={friendClient()} runtimeClient={runtimeClient} conciergeClient={conciergeClient} />));
  const input = document.querySelector<HTMLTextAreaElement>("textarea")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(input, "Lunch on Friday?");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => { document.querySelector<HTMLButtonElement>("[data-testid=composer-send-button]")!.dispatchEvent(new MouseEvent("pointerup", { bubbles: true, button: 0 })); });
  const bubbles = () => Array.from(document.querySelectorAll('.neighbor-msg[data-side="out"]')).filter(item => item.textContent?.includes("Lunch on Friday?"));
  expect(bubbles()).toHaveLength(1);
  expect(bubbles()[0].querySelector('.ct-delivery[data-state="sending"]')).not.toBeNull();
  expect(input.value).toBe("");
  const pending = bubbles()[0];
  vi.mocked(conciergeClient.history).mockResolvedValue({ entries: [{ direction: "out", kind: "question", author: "me", body: "Lunch on Friday?", at: Date.now(), message_id: id, delivery_status: "delivered" }], next_before: null, ended: false });
  await act(async () => accept());
  expect(bubbles()).toHaveLength(1);
  expect(bubbles()[0]).toBe(pending);
  expect(bubbles()[0].id).toBe(`neighbor-msg-${id}`);
  expect(bubbles()[0].querySelector(".ct-delivery")).toBeNull();
});

it("explains a send refused by today's sending limit across all friends", async () => {
  const runtimeClient = runtime();
  vi.mocked(runtimeClient.send).mockRejectedValue(new NeighborRuntimeFailure("unavailable", "Too many", 200,
    { status: "not_delivered", reason: "aggregate_daily_limited", limit: 200, resets_at: Math.floor(Date.now() / 1000) + 3 * 3600 }));
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={client} runtimeClient={runtimeClient} conciergeClient={concierge([])} />));
  const input = document.querySelector<HTMLTextAreaElement>("textarea")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(input, "Hello");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => { document.querySelector<HTMLButtonElement>("[data-testid=composer-send-button]")!.dispatchEvent(new MouseEvent("pointerup", { bubbles: true, button: 0 })); });
  expect(document.body.textContent).toContain("You reached today's sending limit (200). It resets in");
});

it("shows a reply to a stopped request as not sent, without Retry", async () => {
  const runtimeClient = runtime();
  vi.mocked(runtimeClient.send).mockRejectedValue(new NeighborRuntimeFailure("reply_target_stopped", "The owner stopped this request; nothing was sent"));
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [neighbor], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={client} runtimeClient={runtimeClient} conciergeClient={concierge([])} />));
  const input = document.querySelector<HTMLTextAreaElement>("textarea")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(input, "Hello");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => { document.querySelector<HTMLButtonElement>("[data-testid=composer-send-button]")!.dispatchEvent(new MouseEvent("pointerup", { bubbles: true, button: 0 })); });
  expect(document.body.textContent).toContain("The owner stopped this request; nothing was sent");
  expect(document.querySelector(".ct-delivery")?.getAttribute("data-state")).toBe("failed");
  expect(document.querySelector(".ct-delivery__retry")).toBeNull();
});

it("keeps writing open to a friend you disabled and offers Enable", async () => {
  const conciergeClient = await renderConversation({ ...neighbor, my_paused: true });
  expect(document.querySelector<HTMLTextAreaElement>("textarea")!.disabled).toBe(false);
  await act(async () => button("Enable").click());
  expect(conciergeClient.setPaused).toHaveBeenCalledExactlyOnceWith("pair", false);
});

it("shows your concierge working on a friend's message and stops it through the runtime", async () => {
  const conciergeClient = concierge([{ direction: "in", kind: "question", author: "friend", body: "Is the build green?", at: 1700000000000, message_id: "m1",
    handling_state: "working", handling_updated_at: 1700000001000 }]);
  const list: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: [{ ...neighbor, gateway_tab: "tab-gateway" }], incoming: [], outgoing: [] };
  const client: NeighborClient = { list: vi.fn(async () => list), apply: vi.fn(async () => {}) };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={client} runtimeClient={runtime()} conciergeClient={conciergeClient} />));
  expect(document.getElementById("neighbor-msg-m1")?.textContent).toContain("Your concierge is working");
  expect(document.querySelector(".neighbor-chat__working")?.textContent).toContain("Your concierge is working for Ada");
  await act(async () => document.querySelector<HTMLButtonElement>('#neighbor-msg-m1 button[aria-label="Stop your concierge"]')!.click());
  expect(conciergeClient.stopConcierge).toHaveBeenCalledExactlyOnceWith("pair");
});

it("puts a settled concierge outcome in the time row, linked to the reply", async () => {
  const conciergeClient = concierge([
    { direction: "in", kind: "question", author: "friend", body: "Is the build green?", at: 1700000000000, message_id: "m1", handling_state: "answered" },
    { direction: "out", kind: "answer", author: "me", body: "Yes.", at: 1700000002000, message_id: "r1", in_reply_to: "m1" },
  ]);
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={friendClient()} runtimeClient={runtime()} conciergeClient={conciergeClient} />));
  const outcome =document.querySelector("#neighbor-msg-m1 .neighbor-msg__meta .neighbor-work__reply");
  expect(outcome?.textContent).toBe("Answered");
  expect(document.querySelector("#neighbor-msg-m1 .neighbor-msg__stack > .neighbor-work")).toBeNull();
});

it("renders a friend's markdown like agent chat: headings, scrolling tables and copyable code", async () => {
  const body = "## Status\n\n| Job | State |\n| --- | --- |\n| build | green |\n\n```\ncargo test\n```";
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={friendClient()} runtimeClient={runtime()}
    conciergeClient={concierge([{ direction: "in", kind: "question", author: "friend", body, at: 1700000000000, message_id: "md" }])} />));
  const bubble = document.querySelector("#neighbor-msg-md .neighbor-msg__bubble")!;
  expect(bubble.querySelector("h2")?.textContent).toBe("Status");
  expect(bubble.querySelector("table")?.parentElement?.classList.contains("overflow-x-auto")).toBe(true);
  expect(bubble.querySelector('button[aria-label="Copy code"]')).not.toBeNull();
});

it("closes the composer while the friend has disabled messages from you", async () => {
  await renderConversation({ ...neighbor, their_paused: true });
  expect(document.querySelector<HTMLTextAreaElement>("textarea")!.disabled).toBe(true);
  expect(hasButton("Enable")).toBe(false);
});


it("keeps handling lines calm: one waiting line, no raw codes, no line for no reply needed", async () => {
  const conciergeClient = concierge([
    { direction: "in", kind: "question", author: "friend", body: "First", at: 1700000000000, message_id: "w1", handling_state: "waiting", handling_reason: "queued_for_concierge" },
    { direction: "in", kind: "question", author: "friend", body: "Second", at: 1700000001000, message_id: "w2", handling_state: "waiting", handling_reason: "queued_for_concierge" },
    { direction: "in", kind: "question", author: "friend", body: "Thanks!", at: 1700000002000, message_id: "w3", handling_state: "no_reply" },
  ]);
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(<NeighborConversationView friendship="pair" client={friendClient()} runtimeClient={runtime()} conciergeClient={conciergeClient} />));
  const lines = Array.from(document.querySelectorAll(".neighbor-work")).map(line => line.closest(".neighbor-msg")?.id);
  expect(lines).toEqual(["neighbor-msg-w2"]);
  expect(document.body.textContent).not.toContain("queued_for_concierge");
  expect(document.querySelector("#neighbor-msg-w3 time")?.getAttribute("title")).toContain("No reply needed");
});
