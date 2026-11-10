import { channelStatus } from "./neighborRuntime.testdata";
import { it, vi, expect } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { NeighborList } from "./neighborModel";
import type { NeighborClient } from "./neighborClient";
import type { NeighborRuntimeClient, ReceiverState } from "./neighborRuntimeClient";
import { SidebarNeighborsSection } from "./SidebarNeighborsSection";
import { NeighborConversationView } from "./NeighborConversationView";
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
vi.mock("../../store/multiplexer/hooks", () => ({ useMuxSelector: (select: (state: typeof mux) => unknown) => select(mux) }));
// The shared confirmation is Courier's and tested there; here it only has to say which mode it was opened in and confirm.
vi.mock("./NeighborReceiveHere", () => ({
  NeighborReceiveHere: (props: { receiver: ReceiverState; friendName?: string; operation?: string; friendCount?: number; targetMachine?: string; onReceive: (revision: number) => Promise<void> }) =>
    <button type="button" data-friend={props.friendName ?? "all"} data-operation={props.operation ?? "set"} data-count={props.friendCount} data-target={props.targetMachine}
      onClick={() => { void props.onReceive(props.receiver.revision); }}>Confirm receiving</button>,
}));

const { neighbor, mounted, button, hasButton, runtime, concierge, render } = createNeighborsViewFixture({ notices, transport, bridge, account, mux });

const elsewhere: ReceiverState = { receiving_here: false, is_me: false, receiving_enabled: true, receiver_machine_label: "Studio Mac", revision: 7 };
const overridden: ReceiverState = { receiving_here: true, is_me: true, receiving_enabled: true, receiver_machine_label: "This laptop", revision: 3 };

/** The runtime with this account received here, and Ada received as given. */
function receiving(ada: ReceiverState): NeighborRuntimeClient {
  const client = runtime({ pair: "tab-gateway" });
  vi.mocked(client.status).mockResolvedValue(channelStatus({ enrolled: true, bindings: { pair: "tab-gateway" }, machine_label: "This laptop", friend_receivers: { pair: ada } }));
  return { ...client, receiveFriend: vi.fn(async () => {}) };
}
function friends(list: NeighborList["friends"]): NeighborClient {
  const value: NeighborList = { schema_version: 1, access: "paid", can_invite: true, friends: list, incoming: [], outgoing: [] };
  return { list: vi.fn(async () => value), apply: vi.fn(async () => {}) };
}
async function mount(node: React.ReactNode) {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); mounted.current = { root, host };
  await act(async () => root.render(node));
  const toggle = host.querySelector<HTMLButtonElement>('section[aria-label="Neighbors"] button[aria-expanded="false"]');
  if (toggle) await act(async () => toggle.click());
}
/** Opens a row's menu and returns its item labels; `choose` then picks one from the open menu. */
async function openMenu(menu: string): Promise<(string | null)[]> {
  await act(async () => { button(menu).dispatchEvent(new MouseEvent("pointerdown", { bubbles: true, button: 0 })); });
  return Array.from(document.querySelectorAll('[role="menuitem"]')).map(item => item.textContent);
}
async function choose(item: string) {
  const entry = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(element => element.textContent === item);
  if (!entry) throw new Error(`Missing menu item ${item}`);
  await act(async () => entry.click());
}

it("the sidebar moves the receiving line from an existing event and receives only that friend here", async () => {
  const runtimeClient = receiving(overridden);
  const client = friends([neighbor]);
  let receiverChanged: Parameters<NeighborRuntimeClient["subscribe"]>[0] | undefined;
  vi.mocked(runtimeClient.subscribe).mockImplementation(async handler => { receiverChanged = handler; return () => {}; });
  await mount(<SidebarNeighborsSection touchMode={false} client={client} runtimeClient={runtimeClient} />);
  expect(hasButton("Open Ada's concierge")).toBe(true);
  vi.mocked(runtimeClient.status).mockResolvedValue(channelStatus({ enrolled: true, bindings: { pair: "tab-gateway" }, machine_label: "This laptop", friend_receivers: { pair: elsewhere }, friend_receivers_version: 2 }));
  await act(async () => receiverChanged!({ ...elsewhere, friendship_id: "pair", friend_receivers_version: 2 }));
  expect(document.body.textContent).toContain("receiving on Studio Mac");
  expect(hasButton("Open Ada's concierge")).toBe(false);
  expect(client.list).toHaveBeenCalledTimes(1);
  await openMenu("Actions for Ada");
  await choose("Receive this friend here");
  const confirm = button("Confirm receiving");
  expect(confirm.dataset).toMatchObject({ friend: "Ada", operation: "set", target: "This laptop" });
  await act(async () => confirm.click());
  expect(runtimeClient.receiveFriend).toHaveBeenCalledExactlyOnceWith("pair", 7, "set");
});

it("a receiver whose membership expired is shown as the reason the friend is not received, never as the receiver", async () => {
  const lapsed: ReceiverState = { receiving_here: false, receiving_enabled: false, receiver_machine_label: "Studio Mac", revision: 7, is_me: false, not_receiving_reason: "membership_expired" };
  const runtimeClient = receiving(lapsed);
  await mount(<SidebarNeighborsSection touchMode={false} client={friends([neighbor])} runtimeClient={runtimeClient} />);
  expect(document.body.textContent).toContain("not receiving · Studio Mac's membership expired");
  expect(Array.from(document.querySelectorAll<HTMLElement>("[title]"), element => element.title)).toContain("not receiving · Studio Mac's membership expired");
  expect(document.body.textContent).not.toContain("receiving on");
  await act(async () => mounted.current!.root.unmount()); mounted.current!.host.remove(); mounted.current = null;
  await mount(<NeighborConversationView friendship="pair" client={friends([neighbor])} runtimeClient={runtimeClient} conciergeClient={concierge([])} />);
  expect(document.body.textContent).toContain("No machine is receiving Ada's messages: Studio Mac's membership expired.");
  expect(document.body.textContent).not.toContain("messages arrive on");
  await act(async () => button("Receive this friend here").click());
  await act(async () => button("Confirm receiving").click());
  expect(runtimeClient.receiveFriend).toHaveBeenCalledExactlyOnceWith("pair", 7, "set");
});

it("a local receiver offers stopping only that friend", async () => {
  const runtimeClient = receiving(overridden);
  await mount(<SidebarNeighborsSection touchMode={false} client={friends([neighbor])} runtimeClient={runtimeClient} />);
  const items = await openMenu("Actions for Ada");
  expect(items).toContain("Stop receiving this friend");
  expect(items).not.toContain("Receive this friend here");
  await choose("Stop receiving this friend");
  expect(button("Confirm receiving").dataset.operation).toBe("reset");
  await act(async () => button("Confirm receiving").click());
  expect(runtimeClient.receiveFriend).toHaveBeenCalledExactlyOnceWith("pair", 3, "reset");
});


it("the current concierge row's menu stops receiving the same friend here", async () => {
  const runtimeClient = receiving(overridden);
  await mount(<SidebarNeighborsSection touchMode={false} client={friends([neighbor])} runtimeClient={runtimeClient} />);
  await openMenu("Actions for Ada's concierge");
  await choose("Stop receiving this friend");
  await act(async () => button("Confirm receiving").click());
  expect(runtimeClient.receiveFriend).toHaveBeenCalledExactlyOnceWith("pair", 3, "reset");
});

it("the conversation says where the friend is received and offers receiving them here without blocking the chat", async () => {
  const runtimeClient = receiving(elsewhere);
  await mount(<NeighborConversationView friendship="pair" client={friends([neighbor])} runtimeClient={runtimeClient} conciergeClient={concierge([])} />);
  expect(document.body.textContent).toContain("Ada's messages arrive on Studio Mac.");
  expect(document.querySelector<HTMLTextAreaElement>("textarea")!.disabled).toBe(false);
  await act(async () => button("Receive this friend here").click());
  await act(async () => button("Confirm receiving").click());
  expect(runtimeClient.receiveFriend).toHaveBeenCalledExactlyOnceWith("pair", 7, "set");
});

it("the conversation header's menu receives the friend here through the same confirmation", async () => {
  const runtimeClient = receiving(elsewhere);
  await mount(<NeighborConversationView friendship="pair" client={friends([neighbor])} runtimeClient={runtimeClient} conciergeClient={concierge([])} />);
  const items = await openMenu("Actions for Ada");
  expect(items).toContain("Receive this friend here");
  await choose("Receive this friend here");
  expect(button("Confirm receiving").dataset).toMatchObject({ friend: "Ada", operation: "set" });
  await act(async () => button("Confirm receiving").click());
  expect(runtimeClient.receiveFriend).toHaveBeenCalledExactlyOnceWith("pair", 7, "set");
});

it("a friend the status does not know yet borrows nothing from the account: no line, no action, no notice", async () => {
  // The account is received elsewhere, but Ada's own receiver is unknown, so nothing may claim where Ada arrives.
  const unknown = (): NeighborRuntimeClient => {
    const client = receiving(elsewhere);
    vi.mocked(client.status).mockResolvedValue(channelStatus({ enrolled: true, bindings: { pair: "tab-gateway" }, machine_label: "This laptop", friend_receivers: {} }));
    return client;
  };
  await mount(<SidebarNeighborsSection touchMode={false} client={friends([neighbor])} runtimeClient={unknown()} />);
  expect(document.body.textContent).not.toContain("receiving on");
  const sidebarItems = await openMenu("Actions for Ada");
  expect(sidebarItems).not.toContain("Receive this friend here");
  expect(sidebarItems).not.toContain("Stop receiving this friend");
  await act(async () => mounted.current!.root.unmount()); mounted.current!.host.remove(); mounted.current = null;
  await mount(<NeighborConversationView friendship="pair" client={friends([neighbor])} runtimeClient={unknown()} conciergeClient={concierge([])} />);
  expect(document.body.textContent).not.toContain("messages arrive on");
  expect(hasButton("Receive this friend here")).toBe(false);
});

it("offers no account-wide receiver action", async () => {
  await render("paid", { runtimeClient: receiving(elsewhere) });
  expect(document.querySelector('[data-friend="all"]')).toBeNull();
});

it("keeps the single-friend receiving action inside concierge setup", async () => {
  const runtimeClient = receiving(elsewhere);
  await render("paid", { runtimeClient });
  await act(async () => button("Ada, connected").click());
  expect(hasButton("Change agent")).toBe(false);
  await act(async () => button("Set up concierge").click());
  const dialog = document.querySelector('form[role="dialog"]');
  const receive = Array.from(dialog?.querySelectorAll("button") ?? []).find(node => node.textContent === "Confirm receiving");
  expect(receive).toBeDefined();
  await act(async () => receive!.click());
  expect(runtimeClient.receiveFriend).toHaveBeenCalledExactlyOnceWith("pair", 7);
});
