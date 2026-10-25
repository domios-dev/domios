import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import {
  __reloadLocalInstanceNameForTest,
  __resetLocalInstanceNameForTest,
  notifyMeshPeersLoaded,
} from "../../lib/localInstanceName";
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
  return { label, unmount };
}

afterEach(() => {
  __resetLocalInstanceNameForTest();
  peersResponse.mockReset();
  healthResponse.mockReset();
});

it("shows the machine name from health when the first mesh peers load fails", async () => {
  peersResponse.mockImplementation(failing);
  healthResponse.mockImplementation(healthy);
  const chip = await mount();
  expect(chip.label()).toBe("Switch Domios host, current host BLACKSTAR [dev]");
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
  expect(reloaded.label()).toBe("Switch Domios host, current host BLACKSTAR [dev]");
  await reloaded.unmount();
});

it("keeps the instance name when a remounted chip cannot reach the peers endpoint", async () => {
  peersResponse.mockImplementation(serving);
  const first = await mount();
  expect(first.label()).toBe("Switch Domios host, current host BLACKSTAR [dev]");
  await first.unmount();

  peersResponse.mockImplementation(failing);
  const remounted = await mount();
  expect(remounted.label()).toBe("Switch Domios host, current host BLACKSTAR [dev]");
  await remounted.unmount();
});

it("picks up the instance name once mesh peers load after a failed first fetch", async () => {
  peersResponse.mockImplementation(failing);
  healthResponse.mockImplementation(failing);
  const chip = await mount();
  expect(chip.label()).toBe("Switch Domios host, current host Domios");

  peersResponse.mockImplementation(serving);
  await act(async () => { notifyMeshPeersLoaded(); });
  expect(chip.label()).toBe("Switch Domios host, current host BLACKSTAR [dev]");
  await chip.unmount();
});
