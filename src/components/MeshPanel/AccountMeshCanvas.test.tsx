import { __setInvokeForTest } from "../../lib/bridge";
import { ServiceDrawer } from "./ServiceDrawer";
import { PeerAccountFooterView } from "./AccountCanvasNodes";
import { MachineVersion } from "./MachineFooter";
import { accountCanvas } from "./accountCanvas";
import { myMachinesView } from "./accountMachines";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountPresence } from "./accountMachines";
import { AccountMeshClientProvider } from "./accountMeshClient";
import { FakeAccountMeshClient, macRow, NOW, pair, selfRow, snapshot } from "./accountMachines.testdata";
import type { PeerStatus } from "./useMeshData";

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
  useCanvasViewport: () => ({ containerRef: { current: null }, layerRef: { current: null }, transform: { x: 0, y: 0, scale: 1 }, handlers: {}, transformRef: { current: { x: 0, y: 0, scale: 1 } }, setTransform: () => {}, fitToView: () => {}, zoomBy: () => {} }),
}));

const { AccountMeshCanvas } = await import("./AccountMeshCanvas");
const { useMeshConnections } = await import("./useMeshConnections");

const peer = (instanceId: string, name: string, patch: Partial<PeerStatus> = {}): PeerStatus => ({
  instance_id: instanceId, name, ip: "100.64.0.2", api_port: 7685, online: true, access_level: "full", last_seen: null, relay_protocol_version: 2, ...patch,
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

  async function mount(client: FakeAccountMeshClient, peers: PeerStatus[] = [], registryRefreshKey = 0) {
    await act(async () => root.render(
      <AccountMeshClientProvider value={client}>
        <AccountMeshCanvas registryRefreshKey={registryRefreshKey} onAccountChanged={() => { changed++; }} thisMachine={{ name: "Studio PC", ip: "127.0.0.1" }} peers={peers}
          recentPushes={[]} onPeerClick={(name) => { selected++; clickedNames.push(name); }} webUiBaseUrl="http://127.0.0.1:7686" onOpenPeerWebUi={() => { selected++; }} width={1200} height={800} />
      </AccountMeshClientProvider>,
    ));
  }
  const button = (label: string) => host.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);
  const node = (name: string) => host.querySelector<HTMLElement>(`section[aria-label="${name}"], section[aria-label^="${name},"]`);
  const click = async (element: HTMLElement | null) => {
    expect(element).not.toBeNull();
    await act(async () => element!.click());
  };

  it("counts active lifecycle work and clears the overall count on backoff", async () => {
    const state = snapshot({ connections: [{ installation_id: macRow.installation_id, phase: "verifying" }] });
    const client = new FakeAccountMeshClient(state);
    function Count() { return <output aria-label="Peers connecting">{useMeshConnections(true)}</output>; }
    await act(async () => root.render(<AccountMeshClientProvider value={client}><Count /></AccountMeshClientProvider>));
    expect(host.querySelector('output[aria-label="Peers connecting"]')?.textContent).toBe("1");
    await act(async () => client.emit({ ...state, connections: [{ installation_id: macRow.installation_id, phase: "retrying" }] }));
    expect(host.querySelector('output[aria-label="Peers connecting"]')?.textContent).toBe("0");
  });

  it("announces published handshake progress and clears busy state on failure", async () => {
    const state = snapshot({ connections: [{ installation_id: macRow.installation_id, phase: "verifying" }] });
    const client = new FakeAccountMeshClient(state);
    await mount(client);
    expect(node("MacBook")?.querySelector('[role="status"][aria-busy="true"]')).not.toBeNull();
    expect(node("MacBook")?.querySelector('[aria-label="Connecting — verifying identity"]')).not.toBeNull();
    await act(async () => client.emit({ ...state, connections: [{ installation_id: macRow.installation_id, phase: "retrying" }] }));
    expect(node("MacBook")?.querySelector('[aria-busy="true"]')).toBeNull();
    expect(node("MacBook")?.querySelector('[role="status"]')?.textContent).toBe("Unreachable — retry scheduled");
  });

  it.each(["unavailable", "offline", "authority-unavailable"] as const)("restarts stopped %s enrollment through its visible retry and reloads the snapshot", async reason => {
    const refusal = { reason };
    const client = new FakeAccountMeshClient(snapshot({ enrollment: "refused", last_refusal: refusal, retry_at: null, registry: null }));
    const enroll = vi.spyOn(client, "enroll").mockImplementation(async () => {
      client.state = snapshot();
      return { ok: true };
    });
    await mount(client);
    const retry = () => [...host.querySelectorAll<HTMLButtonElement>("button")].find(element => element.textContent === "Retry enrollment") ?? null;
    expect(enroll).not.toHaveBeenCalled();
    expect(node("MacBook")).toBeNull();
    const snapshots = client.snapshotCalls;
    await click(retry());
    expect(enroll).toHaveBeenCalledTimes(1);
    expect(client.snapshotCalls).toBeGreaterThan(snapshots);
    expect(node("MacBook")).not.toBeNull();
    expect(retry()).toBeNull();
  });

  it("keeps enrollment retry hidden while automatic backoff is pending", async () => {
    const client = new FakeAccountMeshClient(snapshot({ enrollment: "refused", last_refusal: { reason: "unavailable" }, retry_at: NOW + 30 }));
    await mount(client);
    expect([...host.querySelectorAll("button")].some(element => element.textContent === "Retry enrollment")).toBe(false);
    expect(client.enrollCalls).toBe(0);
  });

  it("confirms a named machine sign-out on the canvas without removing its pairing", async () => {
    const client = new FakeAccountMeshClient(snapshot());
    await mount(client);
    await act(async () => button("More for MacBook")!.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true })));
    await click([...document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find((entry) => entry.textContent === "Sign out this machine") ?? null);
    expect(client.sessionRevokeCalls).toEqual([]);
    const confirmation = node("MacBook")!.querySelector('[role="group"][aria-label="Sign out MacBook"]');
    expect(confirmation).not.toBeNull();
    await click([...confirmation!.querySelectorAll<HTMLButtonElement>("button")].find((entry) => entry.textContent === "Confirm sign out") ?? null);
    expect(client.sessionRevokeCalls).toEqual([macRow.installation_id]);
    expect(client.revokeCalls).toEqual([]);
  });


  it("refreshes the account registry once on request and renders its name over the saved pair name", async () => {
    const client = new FakeAccountMeshClient(snapshot({ pairs: [pair("mesh-mac", "mac.local", { installation_id: macRow.installation_id })] }));
    const peers = [peer("mesh-mac", "mac.local")];
    await mount(client, peers);
    expect(client.listCalls).toBe(0);
    client.listRows = client.state.registry!.rows.map((row) => row.installation_id === macRow.installation_id
      ? { ...row, display_name: "Travel Mac", metadata_revision: row.metadata_revision + 1 } : row);
    await mount(client, peers, 1);
    expect(client.listCalls).toBe(1);
    expect(button("Rename Travel Mac")).not.toBeNull();
    expect(host.querySelector('[role="button"][aria-label^="Open Travel Mac,"]')).not.toBeNull();
    expect(host.querySelector('[role="button"][aria-label^="Open mac.local,"]')).toBeNull();
    await mount(client, peers, 1);
    await act(async () => client.emit(client.state));
    expect(client.listCalls).toBe(1);
    expect(client.enrollCalls).toBe(0);
  });

  it("renders a peer registry name from a backend notice without another registry read", async () => {
    const client = new FakeAccountMeshClient(snapshot({ pairs: [pair("mesh-mac", "mac.local", { installation_id: macRow.installation_id })] }));
    await mount(client, [peer("mesh-mac", "mac.local")]);
    const rows = client.state.registry!.rows.map((row) => row.installation_id === macRow.installation_id
      ? { ...row, display_name: "Remote rename", metadata_revision: row.metadata_revision + 1 } : row);
    await act(async () => client.emit({ ...client.state, registry: { observed_at: NOW, rows } }));
    expect(button("Rename Remote rename")).not.toBeNull();
    expect(client.listCalls).toBe(0);
  });

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
    await click(host.querySelector('[role="button"][aria-label^="Open Workstation,"]'));
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
    expect(host.querySelector('[role="alert"]')?.textContent).toContain("Busy until ");
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
    expect(host.querySelector('section[aria-label^="MacBook,"]')).not.toBeNull();
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
    await click(host.querySelector('[role="button"][aria-label^="Open Travel Mac,"]'));
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

  it("signs in and rejoins a removed machine with one action", async () => {
    const client = new FakeAccountMeshClient(snapshot({ registry: { observed_at: NOW, rows: [{ ...selfRow, active: false }] }, enrollment: "refused", last_refusal: { reason: "revoked" } }));
    await mount(client);
    const group = host.querySelector('[role="group"][aria-label="Rejoin your mesh"]');
    expect(group).not.toBeNull();
    await click([...group!.querySelectorAll("button")].find(element => element.textContent === "Sign in and rejoin") ?? null);
    expect(presence.signIns).toBe(1);
    expect(client.enrollCalls).toBe(0);
    expect(selected).toBe(0);
    await act(async () => client.emit(snapshot({ self: null, registry: null, enrollment: "refused", last_refusal: { reason: "reauth_required" } })));
    const reauthentication = host.querySelector('[role="group"][aria-label="Add this machine to your mesh"]');
    expect(reauthentication).not.toBeNull();
    await click(reauthentication!.querySelector("button"));
    expect(presence.signIns).toBe(2);
    await act(async () => client.emit(snapshot()));
    expect(host.querySelector('[role="group"][aria-label="Rejoin your mesh"]')).toBeNull();
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




  it("does not offer enrollment retry during rate backoff", async () => {
    const client = new FakeAccountMeshClient(snapshot({ enrollment: "refused", last_refusal: { reason: "rate-limited", retry_after_seconds: 60 }, retry_at: NOW + 60 }));
    await mount(client);
    expect([...host.querySelectorAll("button")].some(element => element.textContent === "Retry enrollment")).toBe(false);
    expect(client.enrollCalls).toBe(0);
    expect(presence.signIns).toBe(0);
  });

  it("disables a signed-out paired card and relay despite the last peer heartbeat being online", async () => {
    const remote = { ...macRow, presence: "signed_out" as const };
    const state = snapshot({ registry: { observed_at: NOW, rows: [selfRow, remote] }, pairs: [pair("mesh-mac", "MacBook", { installation_id: remote.installation_id, online: true })] });
    const client = new FakeAccountMeshClient(state);
    await mount(client, [peer("mesh-mac", "MacBook")]);
    const open = host.querySelector<HTMLElement>('[role="button"][aria-label^="Open MacBook,"]');
    expect(open?.getAttribute("aria-disabled")).toBe("true");
    expect(host.querySelector('[role="group"][aria-label^="MacBook,"]')?.textContent).toContain("Signed out");
    expect(button("Disconnect MacBook")).toBeNull();
    await click(open);
    const relay = host.querySelector<SVGElement>('g[aria-label*="MacBook: Signed out"]');
    expect(relay?.getAttribute("aria-disabled")).toBe("true");
    await act(async () => { relay?.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
    expect(selected).toBe(0);
    await act(async () => client.emit({ ...state, registry: { observed_at: NOW, rows: [selfRow, macRow] } }));
    expect(open?.getAttribute("aria-disabled")).toBe("false");
    await click(open);
    expect(selected).toBe(1);
  });


  it("shows one version and replaces the footer with confirmation, then restores it on Cancel", async () => {
    const client = new FakeAccountMeshClient(snapshot({ pairs: [pair("mesh-mac", "MacBook", { installation_id: macRow.installation_id })] }));
    await mount(client, [peer("mesh-mac", "MacBook", { domios_version: "1.19.2", domios_build: "66576d9" })]);
    const card = host.querySelector('[role="group"][aria-label^="MacBook,"]')!;
    expect(card.textContent?.match(/Connected/g)).toHaveLength(1);
    expect(card.textContent?.match(/Domios 1.19.2/g)).toHaveLength(1);
    const more = button("More for MacBook")!;
    more.focus();
    await act(async () => more.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true })));
    expect(document.activeElement?.textContent).toBe("Change icon and colour");
    await act(async () => document.activeElement!.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true })));
    expect(document.activeElement?.textContent).toBe("Sign out this machine");
    await act(async () => document.activeElement!.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true })));
    expect(document.activeElement?.textContent).toBe("Remove from mesh");
    expect(document.activeElement?.getAttribute("data-danger")).toBe("true");
    await act(async () => document.activeElement!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    expect(document.activeElement).toBe(more);
    expect(more.getAttribute("aria-expanded")).toBe("false");
    await act(async () => more.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true })));
    await click(document.activeElement as HTMLElement);
    expect(card.querySelector('button[aria-label="More for MacBook"]')).toBeNull();
    expect(card.textContent).not.toContain("Domios 1.19.2");
    await click([...card.querySelectorAll<HTMLButtonElement>("button")].find(entry => entry.textContent === "Cancel") ?? null);
    expect(button("More for MacBook")).not.toBeNull();
    expect(card.textContent).toContain("Domios 1.19.2");
    expect(client.revokeCalls).toEqual([]);
  });

  it.each([
    [false, false, "Unreachable", "last seen 2 h ago"],
    [true, false, "Waiting to pair", "pairs when online"],
    [false, true, "Connected", "Its own agents only"],
  ])("presents one state for pending=%s online=%s", async (pending, online, label, secondary) => {
    const remote = peer("mesh-mac", "MacBook", { pending, online, access_level: "agent_isolated", domios_version: "1.20.0" });
    const state = snapshot({ pairs: [pair("mesh-mac", "MacBook", { installation_id: macRow.installation_id, online })] });
    const canvas = accountCanvas({ view: myMachinesView({ account: "signed_in", snapshot: state, now: NOW }), peers: [remote], failures: new Map(), listFailed: false, loadFailed: false });
    await act(async () => root.render(<PeerAccountFooterView footer={canvas.peerFooters.get("mesh-mac")!} peer={remote} name="MacBook" lastSeen={online ? null : "2h ago"} busy={false} actions={{ onPlans: () => {}, onDismiss: () => {} }} />));
    expect(host.querySelectorAll('[role="status"]')).toHaveLength(1);
    expect(host.querySelector('[role="status"]')?.textContent).toBe(`${label} · ${secondary}`);
    expect(button("More for MacBook")).toBeNull();
  });

  it("exposes the local version only on a mismatch dot and keeps build in the native title", async () => {
    await act(async () => root.render(<MachineVersion version="1.19.2" build="66576d9" localVersion="1.20.0" />));
    expect(host.querySelector('[role="img"]')?.getAttribute("aria-label")).toBe("This machine runs 1.20.0");
    expect(host.firstElementChild?.getAttribute("title")).toBe("Build 66576d9");
    expect(host.textContent).toBe("Domios 1.19.2");
    await act(async () => root.render(<MachineVersion version="1.21.0" localVersion="1.20.0" />));
    expect(host.querySelector('[role="img"]')?.getAttribute("title")).toBe("This machine runs 1.20.0");
    await act(async () => root.render(<MachineVersion version="1.20.0" localVersion="1.20.0" />));
    expect(host.querySelector('[role="img"]')).toBeNull();
    await act(async () => root.render(<MachineVersion localVersion="1.20.0" />));
    expect(host.textContent).toBe("");
  });

  it("changes peer access here using human labels and collapses raw failure details", async () => {
    const requests: unknown[] = [];
    let fail = false;
    __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
      expect(command).toBe("mesh_set_access");
      requests.push(args);
      if (fail) throw new Error("opaque backend diagnostic");
      return undefined as T;
    });
    try {
      await act(async () => root.render(<ServiceDrawer state={{ kind: "peer", id: "MacBook" }} services={[]} peers={[peer("mesh-mac", "MacBook")]}
        thisMachineName="Local" onClose={() => {}} onActionDone={() => {}} />));
      const select = host.querySelector<HTMLSelectElement>('select[aria-label="Access for MacBook here"]')!;
      expect([...select.options].map(option => option.textContent)).toEqual(["Full access", "Read only", "Its own agents only"]);
      await act(async () => { select.value = "read_only"; select.dispatchEvent(new Event("change", { bubbles: true })); });
      expect(requests).toEqual([{ instanceId: "mesh-mac", accessLevel: "read_only" }]);
      expect(host.textContent).toContain("MacBook now has Read only here.");
      expect(select.value).toBe("read_only");
      fail = true;
      await act(async () => { select.value = "agent_isolated"; select.dispatchEvent(new Event("change", { bubbles: true })); });
      expect(host.textContent).toContain("Couldn't change MacBook's access. Try again.");
      expect(host.querySelector<HTMLDetailsElement>("details")?.open).toBe(false);
      expect(host.querySelector("details pre")?.textContent).toContain("opaque backend diagnostic");
      expect(host.querySelector('[data-testid="mesh-peer-announcement"]')?.textContent).not.toContain("opaque");
      expect(select.value).toBe("read_only");
    } finally {
      __setInvokeForTest(null);
    }
  });

  it("explains an empty account on the canvas", async () => {
    await mount(new FakeAccountMeshClient(snapshot({ registry: { observed_at: NOW, rows: [selfRow] } })));
    expect(host.textContent).toContain("No other machines yet");
  });
});
