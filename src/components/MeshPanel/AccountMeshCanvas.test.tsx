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
  useCanvasViewport: () => ({ containerRef: { current: null }, layerRef: { current: null }, transform: { x: 0, y: 0, scale: 1 }, handlers: {}, fitToView: () => {}, zoomBy: () => {} }),
}));

const { AccountMeshCanvas } = await import("./AccountMeshCanvas");

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
  const node = (name: string) => host.querySelector<HTMLElement>(`section[aria-label="${name}"]`);
  const click = async (element: HTMLElement | null) => {
    expect(element).not.toBeNull();
    await act(async () => element!.click());
  };

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
    await click(button("Sign out MacBook"));
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
    expect(host.querySelector('[role="button"][aria-label="Open Travel Mac"]')).not.toBeNull();
    expect(host.querySelector('[role="button"][aria-label="Open mac.local"]')).toBeNull();
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

  it("signs in and rejoins a removed machine with one action", async () => {
    const client = new FakeAccountMeshClient(snapshot({ self: null, registry: null, enrollment: "refused", last_refusal: { reason: "revoked" } }));
    await mount(client);
    const group = host.querySelector('[role="group"][aria-label="Rejoin your mesh"]');
    expect(group).not.toBeNull();
    await click(group!.querySelector("button"));
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
    const open = host.querySelector<HTMLElement>('[role="button"][aria-label="Open MacBook"]');
    expect(open?.getAttribute("aria-disabled")).toBe("true");
    expect(host.querySelector('[role="group"][aria-label="MacBook"]')?.textContent).toContain("Signed out");
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


  it("explains an empty account on the canvas", async () => {
    await mount(new FakeAccountMeshClient(snapshot({ registry: { observed_at: NOW, rows: [selfRow] } })));
    expect(host.textContent).toContain("No other machines yet");
  });
});
