import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import {
  __reloadLocalInstanceNameForTest,
  __resetLocalInstanceNameForTest,
  notifyMeshPeersLoaded,
} from "../../lib/localInstanceName";
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

const serving = () => Promise.resolve({ paired: [], local_instance_name: "BLACKSTAR [dev]" });
const healthy = () => Promise.resolve({ instance: { display_name: "BLACKSTAR [dev]" } });
const failing = () => Promise.reject(new Error("request timed out"));

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
});

it("shows the machine name from health when the first mesh peers load fails", async () => {
  peersResponse.mockImplementation(failing);
  healthResponse.mockImplementation(healthy);
  const chip = await mount();
  expect(chip.label()).toBe("Filter sidebar by machine, BLACKSTAR, machine, this machine");
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
  expect(reloaded.label()).toBe("Filter sidebar by machine, BLACKSTAR, machine, this machine");
  await reloaded.unmount();
});

it("keeps the instance name when a remounted chip cannot reach the peers endpoint", async () => {
  peersResponse.mockImplementation(serving);
  const first = await mount();
  expect(first.label()).toBe("Filter sidebar by machine, BLACKSTAR, machine, this machine");
  await first.unmount();

  peersResponse.mockImplementation(failing);
  const remounted = await mount();
  expect(remounted.label()).toBe("Filter sidebar by machine, BLACKSTAR, machine, this machine");
  await remounted.unmount();
});

it("picks up the instance name once mesh peers load after a failed first fetch", async () => {
  peersResponse.mockImplementation(failing);
  healthResponse.mockImplementation(failing);
  const chip = await mount();
  expect(chip.label()).toBe("Filter sidebar by machine, Domios, machine, this machine");

  peersResponse.mockImplementation(serving);
  await act(async () => { notifyMeshPeersLoaded(); });
  expect(chip.label()).toBe("Filter sidebar by machine, BLACKSTAR, machine, this machine");
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
  expect(chip.label()).toBe("Filter sidebar by machine, BLACKSTAR, laptop, this machine");
  await act(async () => chip.host.querySelector<HTMLButtonElement>('[data-testid="peer-picker-button"]')!.click());
  const peer = document.querySelector<HTMLButtonElement>('[data-peer-id="rog"]')!;
  expect(peer.getAttribute("aria-label")).toBe("ROG, desktop PC, offline");
  expect(peer.disabled).toBe(false);
  await act(async () => peer.click());
  expect(chip.label()).toContain("ROG");
  await act(async () => chip.host.querySelector<HTMLButtonElement>('[data-testid="peer-picker-button"]')!.click());
  expect(document.querySelector('[data-peer-id="rog"]')?.getAttribute("aria-current")).toBe("true");
  await act(async () => document.querySelector<HTMLButtonElement>('[data-testid="peer-picker-all"]')!.click());
  expect(chip.label()).toContain("All machines");
  await chip.unmount();
});

it("lists the named local machine first after All and marks its selected pill", async () => {
  peersResponse.mockResolvedValue({
    local_instance_name: "Desk [dev-main]",
    local_machine_identity: { kind: "desktop", color: "blue" },
    paired: [{ instance_id: "peer", instance_name: "Air", online: false }],
  });
  const chip = await mount();
  try {
    const trigger = chip.host.querySelector<HTMLButtonElement>('[data-testid="peer-picker-button"]')!;
    await act(async () => trigger.click());
    const items = document.querySelectorAll('[role="menuitem"]');
    const local = items[1] as HTMLButtonElement;
    expect(local.getAttribute("aria-current")).toBe("true");
    expect(local.getAttribute("aria-label")).toContain("Desk");
    expect(local.textContent).toContain("this");
    expect(local.textContent).not.toContain("online");
    expect(items[2].textContent).toContain("Air");

    await act(async () => local.click());
    expect(trigger.textContent).toContain("Desk");
    expect(trigger.textContent).toContain("this");
    await act(async () => trigger.click());
    expect(document.querySelector('[data-testid="peer-picker-self"]')?.getAttribute("aria-current")).toBe("true");
  } finally { await chip.unmount(); }
});
