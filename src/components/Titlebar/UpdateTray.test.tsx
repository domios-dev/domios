import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { UpdateAvailabilitySnapshot } from "../../lib/updateAvailability";
import { UPDATE_INTENT_KEY } from "../../lib/updatePanelModel";

const fake = vi.hoisted(() => ({
  check: vi.fn(), install: vi.fn(), plugin: vi.fn(), accept: vi.fn(), hold: vi.fn(),
}));
vi.mock("../../lib/updateAvailability", () => ({ checkUpdateAvailability: fake.check }));
vi.mock("../../hooks/useUpdate", () => ({ useUpdate: () => ({ installing: false, error: null, applyUpdate: fake.install }) }));
vi.mock("../../lib/pluginSnapshotStore", () => ({ runPluginSnapshotAction: fake.plugin }));
vi.mock("../../lib/frontendUpdate/browser", () => ({
  acceptFrontendUpdate: fake.accept, setFrontendUpdateHeld: fake.hold,
  frontendUpdateOffer: () => null, subscribeFrontendUpdateOffer: () => () => {},
}));
vi.mock("../../lib/bridge", () => ({ listen: async () => () => {} }));
import { UpdatePanelProvider } from "../../hooks/useUpdatePanel";
import { UpdateTray } from "./UpdateTray";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const snapshot: UpdateAvailabilitySnapshot = { errors: {}, items: [
  { id: "app", kind: "app", installed: "1.0.0", latest: "2.0.0", compatible: true, state: "update_available", reason: "newer", channel: null },
  { id: "plugin", kind: "plugin", installed: "1.0.0", latest: "2.0.0", compatible: true, state: "update_available", reason: "newer", channel: "official" },
  { id: "frontend", kind: "frontend", installed: "1.0.0", latest: "2.0.0", compatible: true, state: "update_available", reason: "newer", channel: null },
  { id: "core", kind: "plugin", installed: "1.0.0", latest: "2.0.0", compatible: true, state: "core_silent", reason: "silent", channel: "core" },
] };
afterEach(() => { localStorage.clear(); vi.clearAllMocks(); });

it("opens grouped offers and the Update all handler persists intent before installation", async () => {
  fake.check.mockResolvedValue(snapshot);
  fake.install.mockImplementation(async () => {
    expect(JSON.parse(localStorage.getItem(UPDATE_INTENT_KEY)!)).toMatchObject({ appTarget: "2.0.0", stage: "app" });
    return { status: "updating" };
  });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => { root.render(<UpdatePanelProvider><UpdateTray /></UpdatePanelProvider>); });
    const trigger = host.querySelector<HTMLButtonElement>('button[aria-label="Update"]')!;
    expect(trigger).not.toBeNull();
    await act(async () => trigger.click());
    const panel = document.querySelector('[role="dialog"][aria-label="Updates"]')!;
    expect(panel).not.toBeNull();
    for (const group of ["App", "Interface", "Plugins"]) expect(panel.querySelector(`section[aria-label="${group} updates"]`)).not.toBeNull();
    const action = [...panel.querySelectorAll("button")].find(button => button.textContent === "Update all")!;
    await act(async () => action.click());
    expect(fake.install).toHaveBeenCalledTimes(1);
    expect(fake.plugin).not.toHaveBeenCalled();
    expect(fake.accept).not.toHaveBeenCalled();
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("reconnects persisted intent to plugin actions after the new host is observed", async () => {
  localStorage.setItem(UPDATE_INTENT_KEY, JSON.stringify({ appTarget: "2.0.0", stage: "app", pluginIds: ["plugin"], interfaceRequested: false }));
  const restarted = { ...snapshot, items: snapshot.items.filter(item => item.kind !== "frontend").map(item => item.kind === "app" ? { ...item, installed: "2.0.0", state: "up_to_date" as const } : item) };
  fake.check.mockResolvedValue(restarted);
  fake.plugin.mockResolvedValue({ plugins: [], channels: [], diff: [] });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<UpdatePanelProvider><UpdateTray /></UpdatePanelProvider>));
    expect(fake.plugin).toHaveBeenCalledExactlyOnceWith({ kind: "update", pluginId: "plugin" });
    expect(fake.install).not.toHaveBeenCalled();
    expect(localStorage.getItem(UPDATE_INTENT_KEY)).toBeNull();
  } finally { await act(async () => root.unmount()); host.remove(); }
});
