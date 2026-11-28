import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { readMachineAppearance } from "../../lib/chatWallpaperClient";
import {
  __reloadLocalInstanceNameForTest,
  __resetLocalInstanceNameForTest,
  notifyMeshPeersLoaded,
} from "../../lib/localInstanceName";
import { DEFAULT_THEME } from "../../lib/themeCss";
import { RELAY_PROTOCOL_VERSION } from "../../lib/types";
import { setSidebarMachineFilter } from "../Sidebar/sidebarMachineFilter";
import { PairedWorkspaceInventoryContext } from "../../store/multiplexer/context";
import { usePairedWorkspaces } from "../../store/multiplexer/usePairedWorkspaces";
import { workspaceBrowseTarget } from "../../lib/workspaceBrowseTarget";
import { WebPeerPicker } from "./WebPeerPicker";
import { applyCloudAvailability } from "../../lib/cloudMachines/availability";

const peersResponse = vi.fn<() => Promise<unknown>>();
const healthResponse = vi.fn<() => Promise<unknown>>();
const bridgeInvoke = vi.fn<(cmd: string, args?: Record<string, unknown>) => Promise<unknown>>();

vi.mock("../../lib/webApi", async (original) => ({
  ...await original<typeof import("../../lib/webApi")>(),
  localJson: (_method: string, path: string) => (path === "/api/health" ? healthResponse() : peersResponse()),
}));
vi.mock("../../lib/bridge", async (original) => ({
  ...await original<typeof import("../../lib/bridge")>(),
  invoke: (cmd: string, args?: Record<string, unknown>) => bridgeInvoke(cmd, args),
  listen: async () => () => {},
}));
vi.mock("../../lib/chatWallpaperClient", () => ({
  readMachineAppearance: vi.fn(),
}));

const serving = () => Promise.resolve({ paired: [], local_instance_name: "BLACKSTAR [dev]" });
const healthy = () => Promise.resolve({ instance: { display_name: "BLACKSTAR [dev]" } });
const failing = () => Promise.reject(new Error("request timed out"));

function press(key: string) {
  return act(async () => {
    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
  });
}

async function mount() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<WebPeerPicker compact />));
  const label = () => host.querySelector("button[data-testid='peer-picker-button']")!.getAttribute("aria-label");
  const unmount = async () => { await act(async () => root.unmount()); host.remove(); };
  return { host, label, unmount };
}

afterEach(() => {
  __resetLocalInstanceNameForTest();
  setSidebarMachineFilter("local");
  peersResponse.mockReset();
  healthResponse.mockReset();
  bridgeInvoke.mockReset();
  vi.mocked(readMachineAppearance).mockReset();
});

it("shows the machine name from health when the first mesh peers load fails", async () => {
  peersResponse.mockImplementation(failing);
  healthResponse.mockImplementation(healthy);
  const chip = await mount();
  expect(chip.label()).toBe("Machine: BLACKSTAR");
  await chip.unmount();
});

it("paints the persisted machine name on a reloaded page while every source fails", async () => {
  peersResponse.mockImplementation(serving);
  const earlier = await mount();
  await earlier.unmount();

  __reloadLocalInstanceNameForTest();
  peersResponse.mockImplementation(failing);
  healthResponse.mockImplementation(failing);
  const reloaded = await mount();
  expect(reloaded.label()).toBe("Machine: BLACKSTAR");
  await reloaded.unmount();
});

it("keeps the instance name when a remounted chip cannot reach the peers endpoint", async () => {
  peersResponse.mockImplementation(serving);
  const first = await mount();
  expect(first.label()).toBe("Machine: BLACKSTAR");
  await first.unmount();

  peersResponse.mockImplementation(failing);
  const remounted = await mount();
  expect(remounted.label()).toBe("Machine: BLACKSTAR");
  await remounted.unmount();
});

it("picks up the instance name once mesh peers load after a failed first fetch", async () => {
  peersResponse.mockImplementation(failing);
  healthResponse.mockImplementation(failing);
  const chip = await mount();
  expect(chip.label()).toBe("Machine: Domios");

  peersResponse.mockImplementation(serving);
  await act(async () => { notifyMeshPeersLoaded(); });
  expect(chip.label()).toBe("Machine: BLACKSTAR");
  await chip.unmount();
});

it("names a paired machine using its announced kind and offline state", async () => {
  peersResponse.mockResolvedValue({
    local_instance_name: "BLACKSTAR",
    local_machine_identity: { kind: "laptop" },
    machine_identities: { rog: { kind: "desktop" } },
    paired: [{ instance_id: "rog", instance_name: "ROG [dev-qa]", addresses: [], api_port: 7685, online: false }],
  });
  const chip = await mount();
  expect(chip.label()).toBe("Machine: BLACKSTAR");
  await act(async () => chip.host.querySelector<HTMLButtonElement>('[data-testid="peer-picker-button"]')!.click());
  const peer = document.querySelector<HTMLButtonElement>('[data-peer-id="rog"]')!;
  expect(peer.getAttribute("aria-label")).toBe("ROG, desktop PC, offline");
  expect(peer.disabled).toBe(false);
  await act(async () => peer.click());
  expect(chip.label()).toContain("ROG");
  await act(async () => chip.host.querySelector<HTMLButtonElement>('[data-testid="peer-picker-button"]')!.click());
  expect(document.querySelector('[data-peer-id="rog"]')?.getAttribute("aria-selected")).toBe("true");
  await act(async () => document.querySelector<HTMLButtonElement>('[data-testid="peer-picker-all"]')!.click());
  expect(chip.label()).toBe("Machine: All machines");
  expect(chip.host.querySelector('[data-testid="peer-picker-button"]')?.textContent).toBe("All machines");
  await chip.unmount();
});

it("keeps a peer's last theme in the menu after it goes offline", async () => {
  const peer = { instance_id: "forest-peer", instance_name: "Forest", relay_protocol_version: RELAY_PROTOCOL_VERSION, online: true };
  peersResponse.mockResolvedValue({ paired: [peer] });
  vi.mocked(readMachineAppearance).mockImplementation(async peerId => ({
    theme: {
      ...DEFAULT_THEME,
      name: peerId === peer.instance_id ? "Forest · Forest Night" : "Local Theme",
      blue: peerId === peer.instance_id ? "#3fb950" : DEFAULT_THEME.blue,
    },
    wallpaper: null,
  }));
  const chip = await mount();
  const trigger = chip.host.querySelector<HTMLButtonElement>('[data-testid="peer-picker-button"]')!;
  const option = () => document.querySelector<HTMLButtonElement>('[data-peer-id="forest-peer"]');
  try {
    await act(async () => trigger.click());
    expect(option()?.getAttribute("aria-label")).toBe("Forest, machine, online");
    expect(option()?.textContent).not.toContain("Forest Night");
    const onlineTone = option()?.style.getPropertyValue("--ct-switcher-tone");
    expect(onlineTone).toMatch(/^#/);
    await act(async () => trigger.click());
    peersResponse.mockResolvedValue({ paired: [{ ...peer, online: false }] });
    await act(async () => notifyMeshPeersLoaded());
    vi.mocked(readMachineAppearance).mockClear();
    await act(async () => trigger.click());
    expect(option()?.getAttribute("aria-label")).toBe("Forest, machine, offline");
    expect(option()?.textContent).not.toContain("Forest Night");
    const offlineTone = option()?.style.getPropertyValue("--ct-switcher-tone");
    expect(offlineTone).toMatch(/^#/);
    expect(offlineTone).not.toBe(onlineTone);
    expect(vi.mocked(readMachineAppearance).mock.calls.map(([id]) => id)).not.toContain(peer.instance_id);
  } finally { await chip.unmount(); }
});

it("offers only machine selections and commits the focused option from the keyboard", async () => {
  vi.mocked(readMachineAppearance).mockResolvedValue({ theme: DEFAULT_THEME, wallpaper: null });
  peersResponse.mockResolvedValue({
    local_instance_name: "Desk [dev-main]",
    local_machine_identity: { kind: "desktop" },
    paired: [{ instance_id: "peer", instance_name: "Air", online: false }],
  });
  const chip = await mount();
  try {
    const trigger = chip.host.querySelector<HTMLButtonElement>('[data-testid="peer-picker-button"]')!;
    expect(trigger.getAttribute("aria-haspopup")).toBe("listbox");
    await act(async () => trigger.click());
    const listbox = document.querySelector<HTMLElement>('[role="listbox"]')!;
    const items = listbox.querySelectorAll<HTMLButtonElement>('[role="option"]');
    expect(items).toHaveLength(3);
    expect(listbox.querySelectorAll("button")).toHaveLength(items.length);
    expect(items[0].getAttribute("aria-label")).toBe("All machines, 2 machines, 1 online");
    expect(items[0].textContent).toContain("2 machines · 1 online");
    const local = items[1];
    expect(local.getAttribute("aria-selected")).toBe("true");
    expect(local.getAttribute("aria-label")).toBe("Desk, desktop PC, this machine, online");
    expect(local.textContent).toContain("this");
    expect(local.textContent).not.toContain("GitHub Dark");
    expect(items[2].textContent).toContain("Air");

    expect(document.activeElement).toBe(local);
    await press("ArrowDown");
    expect(document.activeElement).toBe(items[2]);
    await press("Home");
    expect(document.activeElement).toBe(items[0]);
    await press("Enter");
    expect(trigger.getAttribute("aria-label")).toBe("Machine: All machines");
    expect(document.activeElement).toBe(trigger);

    await act(async () => trigger.click());
    expect(document.activeElement?.getAttribute("data-testid")).toBe("peer-picker-all");
    expect(document.activeElement?.getAttribute("aria-selected")).toBe("true");
    await press("ArrowDown");
    expect(document.activeElement?.getAttribute("data-testid")).toBe("peer-picker-self");
    await press("Escape");
    expect(document.querySelector('[role="listbox"]')).toBeNull();
    expect(document.activeElement).toBe(trigger);
  } finally { await chip.unmount(); }
});

it.each([true, false])("opens Add machine and Open Mesh in keyboard order (paired peers: %s)", async (hasPeers) => {
  peersResponse.mockResolvedValue({
    local_instance_name: "Desk",
    paired: hasPeers ? [{ instance_id: "peer", instance_name: "Air", online: false }] : [],
  });
  const chip = await mount();
  const trigger = chip.host.querySelector<HTMLButtonElement>('[data-testid="peer-picker-button"]')!;
  const meshButton = document.createElement("button");
  document.body.append(meshButton);
  const route = vi.fn((event: Event) => {
    const detail = (event as CustomEvent).detail;
    if (detail.meshTab === "inventory" && !detail.newMachine) meshButton.focus();
  });
  window.addEventListener("codeterm:open-panel", route);
  try {
    await act(async () => trigger.click());
    const options = document.querySelectorAll('[role="listbox"] [role="option"]');
    expect(options).toHaveLength(hasPeers ? 3 : 1);
    const group = document.querySelector('[role="group"][aria-label="Mesh actions"]')!;
    const [add, openMesh] = group.querySelectorAll("button");
    expect(add.textContent?.trim()).toBe("Add machine");
    expect(openMesh.textContent?.trim()).toBe("Open Mesh");
    expect(group.textContent?.includes("Work across your computers")).toBe(!hasPeers);
    expect(document.activeElement).toBe(options[1] ?? options[0]);
    if (hasPeers) {
      await press("ArrowDown");
      expect(document.activeElement).toBe(options[2]);
    }
    await press("ArrowDown");
    expect(document.activeElement).toBe(add);
    await press("ArrowDown");
    expect(document.activeElement).toBe(openMesh);
    await press("ArrowUp");
    expect(document.activeElement).toBe(add);
    await press("Enter");
    expect(route).toHaveBeenCalledTimes(1);
    expect(route.mock.lastCall?.[0]).toMatchObject({ detail: { panel: "mesh", meshTab: "inventory", newMachine: true } });
    expect(document.querySelector('[role="listbox"]')).toBeNull();
    expect(document.activeElement).toBe(trigger);
    expect(chip.label()).toBe("Machine: Desk");

    await act(async () => trigger.click());
    await press("End");
    expect(document.activeElement?.textContent?.trim()).toBe("Open Mesh");
    await press("Enter");
    expect(route).toHaveBeenCalledTimes(2);
    expect(route.mock.lastCall?.[0]).toMatchObject({ detail: { panel: "mesh", meshTab: "inventory" } });
    expect(document.querySelector('[role="group"][aria-label="Mesh actions"]')).toBeNull();
    expect(document.activeElement).toBe(meshButton);
    expect(chip.label()).toBe("Machine: Desk");
  } finally {
    window.removeEventListener("codeterm:open-panel", route);
    meshButton.remove();
    await chip.unmount();
  }
});

it("renders kind=cloud rows with calm cloud states and never puts a usage meter inside the switcher", async () => {
  // The out-of-hours state names cloud commerce, which shows only while cloud machines are enabled.
  applyCloudAvailability(true);
  peersResponse.mockResolvedValue({
    local_instance_name: "Studio",
    paired: [
      {
        instance_id: "cloud-awake",
        instance_name: "Cloud box",
        addresses: [],
        api_port: 7687,
        online: true,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
        kind: "cloud",
        cloud: { kind: "cloud", machine_id: "cm-1", name: "Cloud box", provider: "fly_machines", size: "small", state: "awake", sleep_mode: "stop_billed", uptime_secs: 600, billing_notice: "" },
      },
      {
        instance_id: "cloud-waking",
        instance_name: "Night runner",
        addresses: [],
        api_port: 7687,
        online: false,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
        kind: "cloud",
        cloud: { kind: "cloud", machine_id: "cm-2", name: "Night runner", provider: "fly_machines", size: "large", state: "waking", sleep_mode: "stop_billed", uptime_secs: 0, billing_notice: "" },
      },
    ],
    cloud_machines: [
      { kind: "cloud", machine_id: "cm-3", name: "Render box", provider: "fly_machines", size: "small", state: "asleep", sleep_mode: "stop_billed", uptime_secs: 0, peer_instance_id: "cloud-asleep", billing_notice: "" },
      { kind: "cloud", machine_id: "cm-4", name: "Scratch", provider: "fly_machines", size: "small", state: "out_of_credits", sleep_mode: "stop_billed", uptime_secs: 0, peer_instance_id: "cloud-ooc", billing_notice: "" },
    ],
  });
  const chip = await mount();
  try {
    await act(async () => chip.host.querySelector<HTMLButtonElement>('[data-testid="peer-picker-button"]')!.click());
    const menu = document.querySelector<HTMLElement>('[data-testid="peer-picker-menu"]')!;
    expect(menu.querySelector('[data-testid="cloud-usage-meter"]')).toBeNull();
    expect(menu.querySelector('[data-peer-id="cloud-awake"]')?.getAttribute("aria-label")).toBe("Cloud box, virtual machine, online");
    expect(menu.querySelector('[data-peer-id="cloud-waking"]')?.getAttribute("aria-label")).toBe("Night runner, virtual machine, waking up");
    expect(menu.querySelector('[data-peer-id="cloud-asleep"]')?.getAttribute("aria-label")).toBe("Render box, virtual machine, asleep");
    expect(menu.querySelector('[data-peer-id="cloud-ooc"]')?.getAttribute("aria-label")).toBe("Scratch, virtual machine, asleep, out of cloud hours");
  } finally {
    await chip.unmount();
    applyCloudAvailability(false);
  }
});

it("shows a visible reconnecting state when switching to a reconnecting peer", async () => {
  peersResponse.mockResolvedValue({
    local_instance_name: "BLACKSTAR",
    paired: [{ instance_id: "peer-ubunta", instance_name: "ubunta", addresses: [], api_port: 7685, online: true, relay_protocol_version: RELAY_PROTOCOL_VERSION }],
  });
  setSidebarMachineFilter({ id: "peer-ubunta", name: "ubunta" });
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(
      <PairedWorkspaceInventoryContext.Provider value={{
        generals: [{ peerId: "peer-ubunta", name: "ubunta", availability: "reconnecting", sessions: [] }],
        browse: { target: { id: "peer-ubunta", name: "ubunta" }, availability: "reconnecting", workspaces: [], synced: false },
      }}>
        <WebPeerPicker compact />
      </PairedWorkspaceInventoryContext.Provider>,
    ));
    const trigger = host.querySelector<HTMLButtonElement>('[data-testid="peer-picker-button"]')!;
    expect(trigger.getAttribute("aria-label")).toBe("Machine: ubunta (reconnecting)");
    expect(trigger.getAttribute("data-reconnecting")).toBe("true");
    const status = host.querySelector('[data-testid="peer-picker-reconnecting"]');
    expect(status?.getAttribute("role")).toBe("status");
    expect(status?.textContent).toContain("Reconnecting");

    await act(async () => trigger.click());
    const option = document.querySelector<HTMLButtonElement>('[data-peer-id="peer-ubunta"]')!;
    expect(option.getAttribute("aria-label")).toContain("reconnecting");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("merges an enrolled cloud machine into one online row, persists switching, loads workspaces, and degrades on detach", async () => {
  const peerId = "3efa6b47-cbad-4243-b381-86e57930319a";
  const instId = "e45c9e4ff42143f0bee45f1f75b11a9c";
  const makePeersPayload = (online: boolean) => ({
    local_instance_name: "BLACKSTAR",
    paired: [{
      instance_id: peerId,
      instance_name: "quill-fly-124-go",
      addresses: [],
      api_port: 0,
      public_key: "pk-cloud",
      paired_at: "2026-01-01T00:00:00Z",
      last_seen: null,
      online,
      is_primary: false,
      access_level: "full",
      relay_protocol_version: RELAY_PROTOCOL_VERSION,
      consecutive_misses: 0,
      kind: "cloud",
      cloud: {
        kind: "cloud",
        machine_id: "cloud-DPGKwwYH",
        name: "quill-fly-124-go",
        provider: "fly_machines",
        size: "small",
        state: "awake",
        sleep_mode: "stop",
        uptime_secs: 120,
        billing_notice: "",
      },
    }],
    installations: [{
      installation_id: instId,
      instance_id: peerId,
      name: "quill-fly-124-go",
      active: true,
      signed_out: false,
      online,
    }],
    direct_ingress: {
      [instId]: { address: "wss://domios-quill.fly.dev", stream_attached: online, inbound_only: true },
    },
    cloud_machines: [{
      kind: "cloud",
      machine_id: "cloud-DPGKwwYH",
      name: "quill-fly-124-go",
      provider: "fly_machines",
      size: "small",
      state: "awake",
      sleep_mode: "stop",
      uptime_secs: 120,
      installation_id: instId,
      peer_instance_id: peerId,
      billing_notice: "",
    }],
  });

  const onlinePayload = makePeersPayload(true);
  peersResponse.mockResolvedValue(onlinePayload);
  bridgeInvoke.mockImplementation(async (cmd) => {
    if (cmd === "mesh_peer_mux_state") {
      return {
        workspaces: [
          { id: "ws-general", name: "General", protected: true, tabs: ["tab-ga"], sort_order: 0 },
          { id: "ws-fly", name: "Fly Project", protected: false, tabs: ["tab-work"], sort_order: 1 },
        ],
        tabs: {
          "tab-ga": { id: "tab-ga", workspace_id: "ws-general", title: "General", role: "general_agent", agent_type: "claude", sort_order: 0 },
          "tab-work": { id: "tab-work", workspace_id: "ws-fly", title: "Worker", agent_type: "claude", sort_order: 0 },
        },
        teams: [],
        active_workspace_id: "ws-fly",
        focused_tab_id: "tab-work",
      };
    }
    return [];
  });

  const observed: { browse: ReturnType<typeof usePairedWorkspaces>["browse"] } = { browse: null };
  function Harness({ payload, revision }: { payload: ReturnType<typeof makePeersPayload>; revision: number }) {
    const inventory = usePairedWorkspaces({ peers: payload as never, revision }, true, null);
    observed.browse = inventory.browse;
    return (
      <PairedWorkspaceInventoryContext.Provider value={{
        generals: inventory.generals,
        browse: inventory.browse,
        peerWorkspaces: inventory.peerWorkspaces,
        refreshPeerInventory: inventory.refreshPeerInventory,
      }}>
        <WebPeerPicker compact />
      </PairedWorkspaceInventoryContext.Provider>
    );
  }

  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness payload={onlinePayload} revision={1} />));
    const trigger = host.querySelector<HTMLButtonElement>('[data-testid="peer-picker-button"]')!;
    await act(async () => trigger.click());

    // Exactly one remote row — never a duplicate paired + unpaired cloud pair.
    const remoteItems = document.querySelectorAll<HTMLButtonElement>('[data-testid="peer-picker-item"]');
    expect(remoteItems).toHaveLength(1);
    expect(remoteItems[0].getAttribute("data-peer-id")).toBe(peerId);
    expect(remoteItems[0].getAttribute("aria-label")).toBe("quill-fly-124-go, virtual machine, online");

    // Switching selects the paired instance_id, persists across roster updates, and loads remote workspaces.
    await act(async () => remoteItems[0].click());
    expect(workspaceBrowseTarget()).toEqual({ id: peerId, name: "quill-fly-124-go" });
    expect(trigger.getAttribute("aria-label")).toBe("Machine: quill-fly-124-go");
    expect(observed.browse?.availability).toBe("online");
    expect(observed.browse?.workspaces.map((ws) => ws.workspace.name)).toEqual(["Fly Project"]);
    expect(observed.browse?.workspaces[0].tabs.map((t) => t.tab.id)).toEqual(["tab-work"]);

    // Roster refresh keeps the browse target selected (no bounce back to local).
    await act(async () => root.render(<Harness payload={onlinePayload} revision={2} />));
    expect(workspaceBrowseTarget()?.id).toBe(peerId);
    expect(observed.browse?.availability).toBe("online");

    // Detaching the ingress stream degrades the row to offline instead of staying falsely online.
    const detachedPayload = makePeersPayload(false);
    peersResponse.mockResolvedValue(detachedPayload);
    await act(async () => {
      notifyMeshPeersLoaded();
      root.render(<Harness payload={detachedPayload} revision={3} />);
    });
    await act(async () => trigger.click());
    const detachedItem = document.querySelector<HTMLButtonElement>(`[data-peer-id="${peerId}"]`)!;
    expect(detachedItem.getAttribute("aria-label")).toBe("quill-fly-124-go, virtual machine, offline");
    expect(observed.browse?.availability).toBe("offline");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
