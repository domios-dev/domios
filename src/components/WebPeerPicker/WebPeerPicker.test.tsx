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
import { WebPeerPicker } from "./WebPeerPicker";

const peersResponse = vi.fn<() => Promise<unknown>>();
const healthResponse = vi.fn<() => Promise<unknown>>();

vi.mock("../../lib/webApi", async (original) => ({
  ...await original<typeof import("../../lib/webApi")>(),
  localJson: (_method: string, path: string) => (path === "/api/health" ? healthResponse() : peersResponse()),
}));
vi.mock("../../lib/bridge", async (original) => ({
  ...await original<typeof import("../../lib/bridge")>(),
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
    local_machine_identity: { kind: "laptop", color: "blue" },
    machine_identities: { rog: { kind: "desktop", color: "teal" } },
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
    local_machine_identity: { kind: "desktop", color: "blue" },
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
    if (detail.meshTab === "inventory") meshButton.focus();
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
    expect(route.mock.lastCall?.[0]).toMatchObject({ detail: { panel: "mesh", meshTab: "network" } });
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
