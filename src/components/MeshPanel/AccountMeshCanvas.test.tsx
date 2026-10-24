import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { accountCanvas, lastSeenMs, type AccountCanvasInput } from "./accountCanvas";
import { myMachinesView, SIGNED_OUT_COPY, type AccountPresence } from "./accountMachines";
import { AccountMeshClientProvider, bridgeAccountMeshClient, type AccountMeshSnapshot } from "./accountMeshClient";
import { AccountMeshRefusalError } from "../billing/accountMeshLimit";
import { freeLimitGate } from "../billing/freeLimitGate";
import { deferred, linkRequest, FakeAccountMeshClient, machine, macRow, NOW, pair, selfRow, snapshot, targetOf } from "./accountMachines.testdata";
import type { PeerStatus } from "./useMeshData";
import type { MachineFailure } from "./useMyMachines";

const presence = vi.hoisted(() => ({ current: "signed_in" as AccountPresence, signIns: 0 }));
const relay = vi.hoisted(() => ({ forPeer: vi.fn(), json: vi.fn(async () => ({ ok: true })) }));
vi.mock("../../lib/daemonBootstrap", async (original) => ({
  ...await original<typeof import("../../lib/daemonBootstrap")>(),
  daemonTransportForPeer: (peerId: string) => { relay.forPeer(peerId); return { json: relay.json }; },
}));
vi.mock("./useMeshAccount", () => ({
  useMeshAccount: () => ({ presence: presence.current, signIn: async () => { presence.signIns++; } }),
}));
vi.mock("@codeterm/canvas-camera", async (original) => ({
  ...await original<typeof import("@codeterm/canvas-camera")>(),
  formatLayerTransform: () => "",
  useCanvasViewport: () => ({ containerRef: { current: null }, layerRef: { current: null }, transform: { x: 0, y: 0, scale: 1 }, handlers: {}, fitToView: () => {}, zoomBy: () => {} }),
}));

const { AccountMeshCanvas } = await import("./AccountMeshCanvas");

const peer = (instanceId: string, name: string, patch: Partial<PeerStatus> = {}): PeerStatus => ({
  instance_id: instanceId, name, ip: "100.64.0.2", api_port: 7685, online: true, access_level: "full", last_seen: null, relay_protocol_version: 2, ...patch,
});

function model(account: AccountPresence, state: AccountMeshSnapshot, patch: Partial<AccountCanvasInput> = {}) {
  const view = myMachinesView({ account, snapshot: state, linking: new Set(), now: NOW });
  return accountCanvas({ view, peers: [], loadFailed: false, listFailed: false, failures: new Map(), ...patch });
}

describe("Account machines on the Mesh canvas", () => {
  it("relays rename through the target machine's existing self-rename route", async () => {
    expect(await bridgeAccountMeshClient.renamePeer!("mesh-mac", 17, "Travel Mac")).toEqual({ ok: true });
    expect(relay.forPeer).toHaveBeenCalledWith("mesh-mac");
    expect(relay.json).toHaveBeenCalledWith("POST", "/api/account/mesh/rename", { expected_metadata_revision: 17, display_name: "Travel Mac" });
  });
  it("explains account mesh with a Sign in card when signed out", () => {
    const signedOut = model("signed_out", snapshot({ binding: "none", self: null, registry: null, traffic: "blocked_signed_out" }));
    expect(signedOut.card).toEqual({ kind: "sign_in", headline: SIGNED_OUT_COPY, blocked: true });
    expect(signedOut.nodes).toEqual([]);
  });

  it("shows one empty-account node when no other machine is in the account", () => {
    const empty = model("signed_in", snapshot({ registry: { observed_at: NOW, rows: [selfRow] } }));
    expect(empty.card).toEqual({ kind: "empty", listFailed: false });
    expect(model("signed_in", snapshot({ registry: null }), { listFailed: true }).card).toEqual({ kind: "empty", listFailed: true });
  });

  it("projects a reauthentication refusal without an unavailable or empty-account card", () => {
    const state = snapshot({ self: null, registry: null, enrollment: "refused", last_refusal: { reason: "reauth_required" } });
    const view = myMachinesView({ account: "signed_in", snapshot: state, linking: new Set(), now: NOW });
    expect(view.kind === "account" && view.reauthRequired).toBe(true);
    expect(model("signed_in", state).card).toBeNull();
    expect(view.kind === "account" && view.notes.join(" ")).not.toMatch(/unavailable/i);
  });

  it("puts a connected machine's account state on its paired card and draws the rest as nodes", () => {
    const state = snapshot({ pairs: [pair("mesh-mac", "mac.local", { installation_id: macRow.installation_id, online: false })] });
    const view = myMachinesView({ account: "signed_in", snapshot: state, linking: new Set(), now: NOW });
    const canvas = accountCanvas({ view, peers: [peer("mesh-mac", "MacBook", { online: false })], loadFailed: false, listFailed: false, failures: new Map() });
    expect(canvas.card).toBeNull();
    const footer = canvas.peerFooters.get("mesh-mac");
    expect(footer?.status).toBe("Connected");
    expect(footer?.disconnect).toEqual({ instanceId: "mesh-mac", confirm: false });
    expect(canvas.nodes.map((node) => [node.name, node.status, node.action?.kind])).toEqual([["Lenovo", "Not connected", "connect"]]);
    expect(canvas.nodes[0].lastSeenAt).toBe(NOW - 60);
  });

  it("marks a connecting machine busy and a machine outside the plan as locked with plans", () => {
    const rows = [selfRow, macRow, machine("inst-extra-e", "Spare", { eligibility: "mesh-allowance" })];
    const view = myMachinesView({ account: "signed_in", snapshot: snapshot({ registry: { observed_at: NOW, rows } }), linking: new Set([macRow.installation_id]), now: NOW });
    const canvas = accountCanvas({ view, peers: [], loadFailed: false, listFailed: false, failures: new Map() });
    const [mac, spare] = canvas.nodes;
    expect(mac.action).toEqual({ kind: "connect", target: targetOf(macRow), disabledReason: null, connecting: true });
    expect(spare.locked).toBe(true);
    expect(spare.action).toEqual({ kind: "plans" });
  });

  it("turns a failed connect into a callout on that machine with a retry", () => {
    const failures = new Map<string, MachineFailure>([[macRow.installation_id, { operation: "connect MacBook", failure: { reason: "handshake-failed" } }]]);
    const canvas = model("signed_in", snapshot(), { failures });
    const mac = canvas.nodes.find((node) => node.name === "MacBook");
    expect(mac?.callout).toEqual({ title: "Could not connect MacBook", message: "The machines could not complete an authorized handshake.", retry: true });
    expect(canvas.nodes.find((node) => node.name === "Lenovo")?.callout).toBeNull();
  });

  it("hides removed machines and asks before dropping a pairing that is not in the account", () => {
    const rows = [selfRow, machine("inst-gone-f", "Gone", { active: false, eligibility: "revoked" })];
    const canvas = model("signed_in", snapshot({ registry: { observed_at: NOW, rows }, pairs: [pair("mesh-old", "Old laptop", { admission: "legacy" })] }));
    expect(canvas.nodes.map((node) => node.name)).toEqual(["Old laptop"]);
    expect(canvas.nodes[0].disconnect).toEqual({ instanceId: "mesh-old", confirm: true });
  });

  it("shows signed-out machines without connect, retry or disconnect actions", () => {
    const remote = { ...macRow, presence: "signed_out" as const };
    const state = snapshot({ registry: { observed_at: NOW, rows: [selfRow, remote] } });
    const failures = new Map<string, MachineFailure>([[remote.installation_id, { operation: "connect MacBook", failure: { reason: "handshake-failed" } }]]);
    const canvas = model("signed_in", state, { failures });
    expect(canvas.nodes[0]).toMatchObject({ status: "Signed out", action: null, disconnect: null, callout: null });
    state.pairs = [pair("mesh-mac", "MacBook", { installation_id: remote.installation_id })];
    const paired = model("signed_in", state, { peers: [peer("mesh-mac", "MacBook")], failures }).peerFooters.get("mesh-mac");
    expect(paired).toMatchObject({ status: "Signed out", disconnect: null, callout: null });
  });

  it("reads last seen from epoch seconds, milliseconds or ISO text", () => {
    expect(lastSeenMs(NOW)).toBe(NOW * 1000);
    expect(lastSeenMs(NOW * 1000)).toBe(NOW * 1000);
    expect(lastSeenMs("2027-01-15T08:00:00Z")).toBe(Date.parse("2027-01-15T08:00:00Z"));
    expect(lastSeenMs(null)).toBeNull();
  });
});

describe("Mesh canvas wiring", () => {
  let host: HTMLDivElement;
  let root: Root;
  let changed = 0;
  let selected = 0;
  let clickedNames: string[] = [];

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    presence.current = "signed_in";
    presence.signIns = 0;
    changed = 0;
    selected = 0;
    clickedNames = [];
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  async function mount(client: FakeAccountMeshClient, peers: PeerStatus[] = []) {
    await act(async () => root.render(
      <AccountMeshClientProvider value={client}>
        <AccountMeshCanvas onAccountChanged={() => { changed++; }} thisMachine={{ name: "Studio PC", ip: "127.0.0.1" }} peers={peers}
          recentPushes={[]} onPeerClick={(name) => { selected++; clickedNames.push(name); }} webUiBaseUrl="http://127.0.0.1:7686" onOpenPeerWebUi={() => { selected++; }} width={1200} height={800} />
      </AccountMeshClientProvider>,
    ));
  }
  const button = (label: string) => host.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);
  const node = (name: string) => host.querySelector<HTMLElement>(`section[aria-label="${name}"]`);
  const click = async (element: HTMLElement | null) => {
    expect(element).not.toBeNull();
    await act(async () => element!.click());
  };

  it("offers Sign in from the canvas when signed out", async () => {
    presence.current = "signed_out";
    await mount(new FakeAccountMeshClient(snapshot({ binding: "none", self: null, registry: null })));
    const card = node("Sign in to use Mesh");
    expect(card).not.toBeNull();
    await click([...card!.querySelectorAll("button")].find((element) => element.textContent === "Sign in") ?? null);
    expect(presence.signIns).toBe(1);
  });

  async function editName(name: string, next: string, code: "Enter" | "Escape") {
    await click(button(`Rename ${name}`));
    const input = host.querySelector<HTMLInputElement>(`input[aria-label="Name for ${name}"]`);
    expect(input).not.toBeNull();
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, next);
      input!.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => input!.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, code })));
  }

  it("renames this machine inline with Enter and cancels with Escape", async () => {
    const client = new FakeAccountMeshClient(snapshot());
    await mount(client);
    const name = client.state.self!.display_name ?? "Studio PC";
    const revision = client.state.self!.metadata_revision;
    await editName(name, "Canceled", "Escape");
    expect(client.renameCalls).toEqual([]);
    expect(host.querySelector("input")).toBeNull();
    client.listRows = client.state.registry!.rows.map((row) => row.installation_id === client.state.self!.installation_id ? { ...row, display_name: "Workstation", metadata_revision: row.metadata_revision + 1 } : row);
    await editName(name, " Workstation ", "Enter");
    expect(client.renameCalls).toEqual([[revision, "Workstation"]]);
    expect(host.querySelector("input")).toBeNull();
    expect(button("Rename Workstation")).not.toBeNull();
    expect(selected).toBe(0);
    await click(host.querySelector('[role="button"][aria-label="Open Workstation"]'));
    expect(clickedNames).toEqual(["Studio PC"]);
  });

  it("keeps the rename draft after a refusal and saves it on the next Enter", async () => {
    const client = new FakeAccountMeshClient(snapshot());
    client.renameOutcomes = [{ ok: false, failure: { reason: "conflict" } }, { ok: true }];
    await mount(client);
    const name = client.state.self!.display_name ?? "Studio PC";
    await editName(name, "Workstation", "Enter");
    const input = host.querySelector<HTMLInputElement>("input");
    expect(input?.value).toBe("Workstation");
    expect(host.querySelector('[role="alert"]')?.textContent).toContain("That machine changed since the list was loaded.");
    await act(async () => input!.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, code: "Enter" })));
    expect(client.renameCalls).toHaveLength(2);
    expect(host.querySelector("input")).toBeNull();
  });

  it.each(["Enter", "blur", "Save"])("saves once through %s without admissions while typing", async (method) => {
    const client = new FakeAccountMeshClient(snapshot());
    await mount(client);
    const name = client.state.self!.display_name!;
    const revision = client.state.self!.metadata_revision;
    const reads = client.listCalls;
    await click(button(`Rename ${name}`));
    const input = host.querySelector<HTMLInputElement>(`input[aria-label="Name for ${name}"]`)!;
    const save = [...host.querySelectorAll("button")].find((element) => element.textContent === "Save")!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, " Workstation ");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(client.renameCalls).toHaveLength(0);
    expect(client.listCalls).toBe(reads);
    await act(async () => {
      if (method === "Enter") {
        input.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, code: "Enter" }));
        input.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, code: "Enter" }));
      }
      if (method === "Save") {
        save.click();
        save.click();
      }
      if (method === "blur") {
        input.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
        input.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
      }
    });
    expect(client.renameCalls).toEqual([[revision, "Workstation"]]);
    expect(client.listCalls).toBe(reads);
    expect(button("Rename Workstation")).not.toBeNull();
  });

  it("reads the current revision when saving instead of retaining the editor's opening snapshot", async () => {
    const client = new FakeAccountMeshClient(snapshot());
    await mount(client);
    await click(button("Rename Studio PC"));
    const input = host.querySelector<HTMLInputElement>('input[aria-label="Name for Studio PC"]')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "Desk");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    client.state = { ...client.state, self: { ...client.state.self!, metadata_revision: 7 } };
    await act(async () => input.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, code: "Enter" })));
    expect(client.renameCalls).toEqual([[7, "Desk"]]);
    expect(client.listCalls).toBe(0);
  });

  it("shows the typed rename refusal and retains the draft", async () => {
    const client = new FakeAccountMeshClient(snapshot());
    client.renameOutcomes = [{ ok: false, failure: { reason: "rate-limited", retry_after_seconds: 15 } }];
    await mount(client);
    await editName("Studio PC", "Desk", "Enter");
    expect(host.querySelector('[role="alert"]')?.textContent).toBe("Too many requests. Try again in 15 seconds.");
    expect(host.querySelector<HTMLInputElement>("input")?.value).toBe("Desk");
    expect(client.renameCalls).toHaveLength(1);
  });

  it("does not admit unchanged names or an Escape followed by blur", async () => {
    const client = new FakeAccountMeshClient(snapshot());
    await mount(client);
    const name = client.state.self!.display_name!;
    await click(button(`Rename ${name}`));
    await act(async () => host.querySelector("input")!.dispatchEvent(new FocusEvent("focusout", { bubbles: true })));
    expect(host.querySelector("input")).toBeNull();
    await click(button(`Rename ${name}`));
    const input = host.querySelector("input")!;
    await act(async () => {
      input.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, code: "Escape" }));
      input.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    });
    expect(client.renameCalls).toHaveLength(0);
  });

  it("keeps the startup page bounded and loads the remaining machines on request", async () => {
    const client = new FakeAccountMeshClient(snapshot({ registry: { observed_at: NOW, rows: [selfRow], cursor: { after_installation_id: selfRow.installation_id, registry_revision: 3 } } }));
    client.listRows = [selfRow, macRow];
    await mount(client);
    expect(client.listCalls).toBe(0);
    expect(host.textContent).toContain("Showing part of your account's machines.");
    const loadMore = [...host.querySelectorAll("button")].find((element) => element.textContent === "Load more machines");
    await click(loadMore ?? null);
    expect(client.listCalls).toBe(1);
    expect(host.querySelector('section[aria-label="MacBook"]')).not.toBeNull();
    expect(host.textContent).not.toContain("Showing part of your account's machines.");
  });

  it("relays a connected account machine's rename and disables disconnected machines", async () => {
    const client = new FakeAccountMeshClient(snapshot({ pairs: [pair("mesh-mac", "mac.local", { installation_id: macRow.installation_id })] }));
    await mount(client, [peer("mesh-mac", "mac.local")]);
    expect(button("Rename Lenovo")?.disabled).toBe(true);
    expect(button("Rename Lenovo")?.title).toBe("Connect to rename it, or rename it on that machine");
    client.listRows = client.state.registry!.rows.map((row) => row.installation_id === macRow.installation_id ? { ...row, display_name: "Travel Mac", metadata_revision: row.metadata_revision + 1 } : row);
    await editName("MacBook", "Travel Mac", "Enter");
    expect(client.peerRenameCalls).toEqual([["mesh-mac", macRow.metadata_revision, "Travel Mac"]]);
    expect(client.renameCalls).toEqual([]);
    expect(selected).toBe(0);
    expect(button("Rename Travel Mac")).not.toBeNull();
    expect(client.listCalls).toBe(0);
    await act(async () => client.emit(client.state));
    expect(button("Rename Travel Mac")).not.toBeNull();
    await click(host.querySelector('[role="button"][aria-label="Open Travel Mac"]'));
    expect(clickedNames).toEqual(["mac.local"]);
    await act(async () => client.emit({ ...client.state, account_epoch: 2 }));
    expect(button("Rename MacBook")).not.toBeNull();
  });

  it("disables rename for an offline paired machine", async () => {
    const client = new FakeAccountMeshClient(snapshot({ pairs: [pair("mesh-mac", "mac.local", { installation_id: macRow.installation_id, online: false })] }));
    await mount(client, [peer("mesh-mac", "mac.local", { online: false })]);
    expect(button("Rename MacBook")?.disabled).toBe(true);
    expect(button("Rename MacBook")?.title).toBe("Connect to rename it, or rename it on that machine");
    expect(client.peerRenameCalls).toEqual([]);
  });

  it("offers one reauthentication action on this machine and waits for backend enrollment", async () => {
    const client = new FakeAccountMeshClient(snapshot({ self: null, registry: null, enrollment: "refused", last_refusal: { reason: "reauth_required" } }));
    await mount(client);
    const card = host.querySelector('[role="group"][aria-label="Add this machine to your mesh"]');
    expect(card).not.toBeNull();
    expect(card!.querySelectorAll("button")).toHaveLength(1);
    await click(card!.querySelector("button"));
    expect(presence.signIns).toBe(1);
    expect(client.enrollCalls).toBe(0);
    expect(selected).toBe(0);
    await act(async () => client.emit(snapshot()));
    expect(host.querySelector('[role="group"][aria-label="Add this machine to your mesh"]')).toBeNull();
  });

  it("connects a machine from its node and shows progress until both machines finish", async () => {
    const client = new FakeAccountMeshClient(snapshot());
    await mount(client);
    expect(node("MacBook")?.getAttribute("data-status")).toBe("unlinked");
    await click(button("Connect MacBook"));
    expect(client.linkCalls).toEqual([targetOf(macRow)]);
    expect(button("Connect MacBook")?.getAttribute("aria-busy")).toBe("true");
    await act(async () => client.linkOutcome.resolve());
    await act(async () => client.emit(snapshot({ pairs: [pair("mesh-mac", "mac.local", { installation_id: macRow.installation_id })] })));
    expect(node("MacBook")?.getAttribute("data-status")).toBe("linked");
    expect(changed).toBeGreaterThan(0);
  });

  it("shows a refused connect as a callout on the node, retries it and dismisses it", async () => {
    const client = new FakeAccountMeshClient(snapshot());
    await mount(client);
    await click(button("Connect MacBook"));
    await act(async () => client.linkOutcome.reject(new AccountMeshRefusalError({ requestId: "req-1", refusal: { reason: "peer-unreachable" } })));
    const alert = node("MacBook")?.querySelector('[role="alert"]');
    expect(alert?.textContent).toContain("Could not connect MacBook");
    expect(alert?.textContent).toContain("Both machines must be online.");
    expect(node("Lenovo")?.querySelector('[role="alert"]')).toBeNull();

    client.linkOutcome = deferred<void>();
    await click([...alert!.querySelectorAll("button")].find((element) => element.textContent?.includes("Try again")) ?? null);
    expect(client.linkCalls).toHaveLength(2);
    expect(node("MacBook")?.querySelector('[role="alert"]')).toBeNull();

    await act(async () => client.linkOutcome.reject(new AccountMeshRefusalError({ requestId: "req-2", refusal: { reason: "handshake-failed" } })));
    await click(button("Dismiss: Could not connect MacBook"));
    expect(node("MacBook")?.querySelector('[role="alert"]')).toBeNull();
  });

  it("disables a signed-out paired card and relay despite the last peer heartbeat being online", async () => {
    const remote = { ...macRow, presence: "signed_out" as const };
    const state = snapshot({ registry: { observed_at: NOW, rows: [selfRow, remote] }, pairs: [pair("mesh-mac", "MacBook", { installation_id: remote.installation_id, online: true })] });
    const client = new FakeAccountMeshClient(state);
    await mount(client, [peer("mesh-mac", "MacBook")]);
    const open = host.querySelector<HTMLElement>('[role="button"][aria-label="Open MacBook"]');
    expect(open?.getAttribute("aria-disabled")).toBe("true");
    expect(host.querySelector('[role="group"][aria-label="MacBook"]')?.textContent).toContain("Signed out");
    expect(button("Disconnect MacBook")).toBeNull();
    await click(open);
    const relay = host.querySelector<SVGElement>('g[aria-label*="MacBook: Signed out"]');
    expect(relay?.getAttribute("aria-disabled")).toBe("true");
    await act(async () => { relay?.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
    expect(selected).toBe(0);
    expect(client.linkCalls).toEqual([]);
    await act(async () => client.emit({ ...state, registry: { observed_at: NOW, rows: [selfRow, macRow] } }));
    expect(open?.getAttribute("aria-disabled")).toBe("false");
    await click(open);
    expect(selected).toBe(1);
  });

  it("keeps the initiator waiting after the request is accepted and restores it from snapshots", async () => {
    const client = new FakeAccountMeshClient(snapshot({ link_requests: [linkRequest()] }));
    await mount(client);
    expect(node("MacBook")?.textContent).toContain("Waiting for approval on MacBook");
    expect(button("Connect MacBook")?.disabled).toBe(true);
    expect(button("Disconnect MacBook")).toBeNull();
  });

  it.each([true, false])("answers the receiver request from its node (%s)", async (approve) => {
    const client = new FakeAccountMeshClient(snapshot({ link_requests: [linkRequest({ incoming: true, notification_id: "notify-1" })] }));
    await mount(client);
    const card = node("MacBook")!;
    const label = approve ? "Approve" : "Decline";
    await click([...card.querySelectorAll("button")].find((button) => button.textContent === label) ?? null);
    expect(client.answeredLinks).toEqual([["notify-1", approve]]);
    expect(client.linkCalls).toHaveLength(0);
  });

  it.each(["declined", "expired"] as const)("offers Try again after the receiver request is %s", async (decision) => {
    const client = new FakeAccountMeshClient(snapshot({ link_requests: [linkRequest({ decision })] }));
    await mount(client);
    await click(button("Try again to connect MacBook"));
    expect(client.linkCalls).toEqual([targetOf(macRow)]);
    await act(async () => client.linkOutcome.resolve());
  });

  it("disconnects a paired machine in one click from its card footer", async () => {
    const client = new FakeAccountMeshClient(snapshot({ pairs: [pair("mesh-mac", "mac.local", { installation_id: macRow.installation_id, online: false })] }));
    await mount(client, [peer("mesh-mac", "MacBook", { online: false, last_seen: String(NOW - 7200) })]);
    expect(node("MacBook")).toBeNull();
    expect(host.textContent).toContain("Last seen");
    await click(button("Disconnect MacBook"));
    expect(client.unpairCalls).toEqual(["mesh-mac"]);
  });

  it("locks a machine outside the plan and opens plans instead of connecting", async () => {
    const spare = machine("inst-extra-e", "Spare", { eligibility: "mesh-allowance" });
    const client = new FakeAccountMeshClient(snapshot({ registry: { observed_at: NOW, rows: [selfRow, spare] } }));
    await mount(client);
    expect(node("Spare")?.querySelector('[data-testid="lock-badge"]')).not.toBeNull();
    expect(button("Connect Spare")).toBeNull();
    try {
      await click(button("See plans for Spare"));
      expect(freeLimitGate.current()?.key).toBe("mesh.machines");
    } finally {
      freeLimitGate.close();
    }
  });

  it("explains an empty account on the canvas", async () => {
    await mount(new FakeAccountMeshClient(snapshot({ registry: { observed_at: NOW, rows: [selfRow] } })));
    expect(host.textContent).toContain("No other machines yet");
  });
});
