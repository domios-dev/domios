import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { UpdateAvailabilitySnapshot } from "../../lib/updateAvailability";
import { UPDATE_INTENT_KEY } from "../../lib/updatePanelModel";

const fake = vi.hoisted(() => ({
  offer: null as { releaseId: string; version: string } | null, check: vi.fn(), install: vi.fn(), plugin: vi.fn(), accept: vi.fn(), hold: vi.fn(),
  availabilityChanged: null as ((event: { payload: UpdateAvailabilitySnapshot }) => void) | null,
}));
vi.mock("../../lib/updateAvailability", () => ({ checkUpdateAvailability: fake.check }));
vi.mock("../../hooks/useUpdate", () => ({ useUpdate: () => ({ installing: false, error: null, applyUpdate: fake.install }) }));
vi.mock("../../lib/pluginSnapshotStore", () => ({ runPluginSnapshotAction: fake.plugin }));
vi.mock("../../lib/frontendUpdate/browser", () => ({
  acceptFrontendUpdate: fake.accept, setFrontendUpdateHeld: fake.hold,
  frontendUpdateOffer: () => fake.offer, subscribeFrontendUpdateOffer: () => () => {},
}));
vi.mock("../../lib/bridge", () => ({ listen: async (_event: string, handler: typeof fake.availabilityChanged) => {
  fake.availabilityChanged = handler;
  return () => { fake.availabilityChanged = null; };
} }));
import { UpdatePanelProvider } from "../../hooks/useUpdatePanel";
import { UpdateTray } from "./UpdateTray";
import { UpdatePanelContents } from "../UpdatePanel";
import { classifyUpdateStatus } from "../../lib/updateStatusModel";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const snapshot: UpdateAvailabilitySnapshot = { errors: {}, items: [
  { id: "app", kind: "app", installed: "1.0.0", latest: "2.0.0", compatible: true, state: "update_available", reason: "newer", channel: null },
  { id: "plugin", kind: "plugin", installed: "1.0.0", latest: "2.0.0", compatible: true, state: "update_available", reason: "newer", channel: "official" },
  { id: "frontend", kind: "frontend", installed: "1.0.0", latest: "2.0.0", compatible: true, state: "update_available", reason: "newer", channel: null },
  { id: "core", kind: "plugin", installed: "1.0.0", latest: "2.0.0", compatible: true, state: "core_silent", reason: "silent", channel: "core" },
] };
afterEach(() => { localStorage.clear(); fake.offer = null; vi.clearAllMocks(); });

for (const failure of [null, "channel download failed"]) {
  it(`wires local-ahead information and ${failure ? "a hidden background failure" : "no failure badge"}`, async () => {
    const ahead: UpdateAvailabilitySnapshot = { items: [{ ...snapshot.items[3], id: "codex", installed: "2.0.0", latest: "1.0.0", reason: "local_ahead" }],
      errors: { core: `system plugin 'codex' is not installable: local_ahead${failure ? `; ${failure}` : ""}` } };
    fake.check.mockResolvedValue(ahead);
    const host = document.createElement("div"); document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<UpdatePanelProvider><UpdateTray /><UpdatePanelContents /></UpdatePanelProvider>));
      expect(host.querySelector('[role="status"][aria-label="Installed plugin ahead of channel"]')?.textContent)
        .toBe(classifyUpdateStatus(ahead).information[0]);
      const trigger = host.querySelector<HTMLButtonElement>('button[aria-label="Update"]');
      expect(trigger).toBeNull();
      expect(host.querySelector('[role="alert"]')).toBeNull();
      if (failure) {
        const check = [...host.querySelectorAll("button")].find(button => button.textContent === "Check again")!;
        await act(async () => check.click());
        expect(fake.check).toHaveBeenLastCalledWith(true);
        expect(host.querySelector('[role="alert"]')).not.toBeNull();
        await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Update"]')!.click());
        const dialog = document.querySelector('[role="dialog"][aria-label="Updates"]')!;
        expect(dialog).not.toBeNull();
        const details = dialog.querySelector("details")!;
        expect(details.open).toBe(false);
        expect(details.querySelector("summary")?.textContent).toBe("Details");
        expect(details.querySelector("pre")?.textContent).toContain(failure);
      }
    } finally { await act(async () => root.unmount()); host.remove(); }
  });
}

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

for (const actionName of ["Apply Interface update", "Update all"]) {
  it(`offers a staged Interface and consents once through ${actionName}`, async () => {
    fake.offer = { releaseId: "frontend-v2.0.0", version: "2.0.0" };
    const staged: UpdateAvailabilitySnapshot = { errors: {}, items: [
      { ...snapshot.items[0], latest: "1.0.0", state: "up_to_date" },
      { ...snapshot.items[2], installed: null, state: "ready_to_apply" },
    ] };
    fake.check.mockResolvedValue(staged);
    const host = document.createElement("div"); document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<UpdatePanelProvider><UpdateTray /></UpdatePanelProvider>));
      expect(fake.check).toHaveBeenCalledWith();
      await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Update"]')!.click());
      const panel = document.querySelector('[role="dialog"][aria-label="Updates"]')!;
      expect(panel.querySelector('section[aria-label="Interface updates"]')).not.toBeNull();
      const action = [...panel.querySelectorAll("button")].find(button => (button.getAttribute("aria-label") ?? button.textContent) === actionName)!;
      expect(action).not.toBeNull();
      expect(action.disabled).toBe(false);
      await act(async () => action.click());
      expect(fake.accept).toHaveBeenCalledTimes(1);
      expect(fake.install).not.toHaveBeenCalled();
      expect(fake.plugin).not.toHaveBeenCalled();
      expect(localStorage.getItem(UPDATE_INTENT_KEY)).toBeNull();
    } finally { await act(async () => root.unmount()); host.remove(); }
  });
}

it("resumes the next plugin from availability values and ignores equivalent snapshots during an action", async () => {
  localStorage.setItem(UPDATE_INTENT_KEY, JSON.stringify({ appTarget: "2.0.0", stage: "app", pluginIds: ["plugin", "second"], interfaceRequested: false }));
  const available: UpdateAvailabilitySnapshot = { ...snapshot, items: [
    ...snapshot.items.filter(item => item.kind !== "frontend"),
    { ...snapshot.items[1], id: "second" },
  ] };
  fake.check.mockResolvedValue(available);
  let release: () => void = () => { throw new Error("plugin action was not started"); };
  fake.plugin.mockImplementationOnce(() => new Promise<void>(resolve => { release = resolve; }))
    .mockResolvedValueOnce({ plugins: [], channels: [], diff: [] });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const resumed: UpdateAvailabilitySnapshot = { ...available, items: available.items.map(item => item.kind === "app"
    ? { ...item, installed: "2.0.0", state: "up_to_date" } : item) };
  try {
    await act(async () => root.render(<UpdatePanelProvider><UpdateTray /></UpdatePanelProvider>));
    expect(fake.plugin).not.toHaveBeenCalled();
    expect(fake.availabilityChanged).not.toBeNull();
    await act(async () => fake.availabilityChanged!({ payload: resumed }));
    expect(fake.plugin).toHaveBeenCalledExactlyOnceWith({ kind: "update", pluginId: "plugin" });
    await act(async () => fake.availabilityChanged!({ payload: structuredClone(resumed) }));
    expect(fake.plugin).toHaveBeenCalledTimes(1);
    fake.check.mockResolvedValue(resumed);
    await act(async () => release());
    expect(fake.plugin.mock.calls).toEqual([[{ kind: "update", pluginId: "plugin" }], [{ kind: "update", pluginId: "second" }]]);
    expect(localStorage.getItem(UPDATE_INTENT_KEY)).toBeNull();
    await act(async () => fake.availabilityChanged!({ payload: structuredClone(resumed) }));
    expect(fake.plugin).toHaveBeenCalledTimes(2);
    expect(fake.install).not.toHaveBeenCalled();
  } finally { await act(async () => root.unmount()); host.remove(); }
});

for (const origin of ["startup", "event"] as const) {
  it(`keeps ${origin} check failures invisible and recovers on subsequent availability`, async () => {
    if (origin === "startup") fake.check.mockRejectedValue(new Error("invalid daemon JSON"));
    else fake.check.mockResolvedValue({ items: [], errors: {} });
    const host = document.createElement("div"); document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<UpdatePanelProvider><UpdateTray /><UpdatePanelContents /></UpdatePanelProvider>));
      if (origin === "event") await act(async () => fake.availabilityChanged!({ payload: { items: [], errors: { app: "offline" } } }));
      expect(host.querySelector('button[aria-label="Update"]')).toBeNull();
      expect(host.querySelector('[role="alert"]')).toBeNull();
      expect(host.querySelector("details")).toBeNull();
      await act(async () => fake.availabilityChanged!({ payload: snapshot }));
      expect(host.querySelector('button[aria-label="Update"]')).not.toBeNull();
      expect(host.querySelector('[role="alert"]')).toBeNull();
    } finally { await act(async () => root.unmount()); host.remove(); }
  });
}

it("shows explicit transport and update failures through the model with collapsed diagnostics", async () => {
  fake.check.mockResolvedValue(snapshot);
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<UpdatePanelProvider><UpdateTray /></UpdatePanelProvider>));
    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Update"]')!.click());
    const dialog = document.querySelector('[role="dialog"][aria-label="Updates"]')!;
    const button = (name: string) => [...dialog.querySelectorAll("button")].find(button => button.textContent === name)!;
    const failure = new Error("InvalidJsonResponseError: /api/update/availability");
    fake.check.mockRejectedValue(failure);
    await act(async () => button("Check again").click());
    expect(dialog.querySelector('[role="alert"]')).not.toBeNull();
    expect(dialog.querySelector("details")?.open).toBe(false);
    expect(dialog.querySelector("pre")?.textContent).toBe(String(failure));
    fake.check.mockResolvedValue(snapshot);
    await act(async () => button("Check again").click());
    expect(dialog.querySelector('[role="alert"]')).toBeNull();
    fake.check.mockRejectedValue(failure);
    await act(async () => button("Update all").click());
    expect(dialog.querySelector('[role="alert"]')).not.toBeNull();
    expect(dialog.querySelector("details")?.open).toBe(false);
    expect(dialog.querySelector("pre")?.textContent).toBe(String(failure));
  } finally { await act(async () => root.unmount()); host.remove(); }
});

for (const kind of ["app", "plugin"] as const) {
  it(`keeps a clicked ${kind} update failure visible through the model`, async () => {
    const item = snapshot.items.find(item => item.kind === kind)!;
    fake.check.mockResolvedValue({ items: kind === "app" ? [item] : [item, { ...snapshot.items[0], latest: "1.0.0", state: "up_to_date" }], errors: {} });
    const failure = new Error("opaque action refusal");
    if (kind === "app") fake.install.mockRejectedValueOnce(failure);
    else fake.plugin.mockRejectedValueOnce(failure);
    const host = document.createElement("div"); document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<UpdatePanelProvider><UpdateTray /></UpdatePanelProvider>));
      await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Update"]')!.click());
      const dialog = document.querySelector('[role="dialog"][aria-label="Updates"]')!;
      const apply = dialog.querySelector<HTMLButtonElement>(`button[aria-label="Apply ${kind === "app" ? "Domios" : "plugin"} update"]`)!;
      expect(apply).not.toBeNull();
      await act(async () => apply.click());
      expect(kind === "app" ? fake.install : fake.plugin).toHaveBeenCalledTimes(1);
      expect(dialog.querySelector('[role="alert"]')).not.toBeNull();
      expect(dialog.querySelector("details")?.open).toBe(false);
      expect(dialog.querySelector("pre")?.textContent).toBe(String(failure));
    } finally { await act(async () => root.unmount()); host.remove(); }
  });
}
