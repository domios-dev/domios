import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { UpdateAvailabilitySnapshot } from "../lib/updateAvailability";

const fake = vi.hoisted(() => ({ check: vi.fn(), plugin: vi.fn(), app: { installing: false, error: null as string | null, currentVersion: "1.0.0", phase: "idle", nativeStatusObserved: false, applyUpdate: vi.fn(), captureFailureBoundary: () => ({ frontendRevision: 0, nativeRevision: null }) }, changed: null as ((event: { payload: UpdateAvailabilitySnapshot }) => void) | null }));
vi.mock("../lib/updateAvailability", () => ({ checkUpdateAvailability: fake.check }));
vi.mock("../hooks/useUpdate", () => ({ useUpdate: () => fake.app }));
vi.mock("../lib/pluginSnapshotStore", () => ({ runPluginSnapshotAction: fake.plugin, ensurePluginSnapshot: async () => ({}), usePluginSnapshotStore: () => ({ snapshot: { plugins: [], marketplace: [] } }) }));
vi.mock("../lib/frontendUpdate/browser", () => ({ acceptFrontendUpdate: vi.fn(), setFrontendUpdateHeld: vi.fn(), frontendUpdateOffer: () => null, subscribeFrontendUpdateOffer: () => () => {} }));
vi.mock("../lib/bridge", () => ({ listen: async (_event: string, handler: typeof fake.changed) => { fake.changed = handler; return () => {}; } }));
import { UpdatePanelProvider, useUpdatePanel } from "../hooks/useUpdatePanel";
import { UpdatePanel } from "./UpdatePanel";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const available: UpdateAvailabilitySnapshot = { errors: {}, items: ["git", "grok"].map(id => ({ id, kind: "plugin", installed: "1.0.0", latest: "2.0.0", compatible: true, state: "update_available", reason: "newer", channel: "official" })) };
afterEach(() => { localStorage.clear(); vi.resetAllMocks(); Object.assign(fake.app, { installing: false, error: null, currentVersion: "1.0.0", phase: "idle", nativeStatusObserved: false }); });

for (const outcome of ["success", "failure", "external"] as const) {
  it(`locks actions and retains component outcomes after ${outcome}`, async () => {
    fake.check.mockResolvedValue(available);
    let resolve!: () => void;
    let reject!: (error: Error) => void;
    fake.plugin.mockImplementationOnce(() => new Promise<void>((yes, no) => { resolve = yes; reject = no; })).mockResolvedValue(undefined);
    const host = document.createElement("div"); document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () => root.render(<UpdatePanelProvider><UpdatePanel /></UpdatePanelProvider>));
      const bulk = host.querySelector<HTMLButtonElement>('[aria-label="Update all"]')!;
      const check = [...host.querySelectorAll("button")].find(button => button.textContent === "Check again")!;
      await act(async () => bulk.click());
      expect(fake.plugin).toHaveBeenCalledExactlyOnceWith({ kind: "update", pluginId: "git" });
      expect(host.querySelector('[aria-label="git update active"]')).not.toBeNull();
      expect(host.querySelector('[aria-label="grok update queued"]')).not.toBeNull();
      expect(host.querySelector('progress')?.hasAttribute("value")).toBe(false);
      expect(bulk.disabled).toBe(true);
      expect(check.disabled).toBe(true);
      expect(host.querySelector('button[aria-label^="Update "]:not([aria-label="Update all"]):not(:disabled)')).toBeNull();
      await act(async () => { bulk.click(); check.click(); });
      expect(fake.plugin).toHaveBeenCalledTimes(1);
      if (outcome === "external") {
        const installed: UpdateAvailabilitySnapshot = { errors: {}, items: available.items.map(item => ({ ...item, installed: "2.0.0", state: "up_to_date", reason: "same_version" })) };
        fake.check.mockResolvedValue(installed);
        await act(async () => fake.changed!({ payload: installed }));
        await act(async () => reject(new Error("already updated by another action")));
      } else if (outcome === "failure") await act(async () => reject(new Error("opaque refusal")));
      else await act(async () => resolve());
      if (outcome === "failure") {
        expect(host.querySelector('[aria-label="git update failed"]')).not.toBeNull();
        expect(host.querySelector('[role="alert"]')).not.toBeNull();
      } else {
        for (const id of ["git", "grok"]) expect(host.querySelector(`[aria-label="${id} update completed"]`)).not.toBeNull();
        expect(host.querySelector('[aria-label$="update active"]')).toBeNull();
        expect(host.querySelector('progress')).toBeNull();
        expect(fake.plugin).toHaveBeenCalledTimes(outcome === "external" ? 1 : 2);
      }
      expect(check.disabled).toBe(false);
      expect(localStorage.getItem("codeterm.update-all.intent")).toEqual(outcome === "failure" ? expect.any(String) : null);
      expect(fake.check).toHaveBeenLastCalledWith(true);
    } finally { await act(async () => root.unmount()); host.remove(); }
  });
}

it("retains failed diagnostics after checking and never offers an update for the removed install", async () => {
  fake.check.mockResolvedValue(available);
  fake.plugin.mockRejectedValueOnce(new Error("opaque final rename refusal"));
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<UpdatePanelProvider><UpdatePanel /></UpdatePanelProvider>));
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Update git"]')!.click());
    expect(host.querySelector('[role="alert"]')?.getAttribute("aria-label")).toContain("git");
    expect(host.querySelector("details")?.open).toBe(false);
    fake.check.mockResolvedValue({ items: [], errors: {} });
    await act(async () => [...host.querySelectorAll("button")].find(button => button.textContent === "Check again")!.click());
    expect(host.querySelector('[aria-label="git update failed"]')).toBeNull();
    expect(host.querySelector('button[aria-label^="Update "]:not([aria-label="Update all"])')).toBeNull();
    expect(host.querySelector<HTMLButtonElement>('[aria-label="Update all"]')?.disabled).toBe(true);
    expect(host.querySelector("pre")?.textContent).toContain("opaque final rename refusal");
    expect(fake.plugin).toHaveBeenCalledTimes(1);
  } finally { await act(async () => root.unmount()); host.remove(); }
});

function expectNoWarningState(host: Element) {
  expect(host.querySelector('[role="alert"]')).toBeNull();
  expect(host.querySelector('[aria-label="Update source unavailable"]')).toBeNull();
  expect(host.querySelector('[aria-label$="update source unavailable"]')).toBeNull();
  expect(host.querySelector('[aria-label$="channel unavailable"]')).toBeNull();
  expect(host.textContent).not.toContain("Can't check");
  expect(host.textContent).not.toContain("Cached update remains available");
}

it("keeps a plugin channel failure quiet: last known rows, no warning, the cause only in closed details", async () => {
  const message = "opaque channel diagnostic";
  fake.check.mockResolvedValue({ ...available, errors: { official: message } });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<UpdatePanelProvider><UpdatePanel /></UpdatePanelProvider>));
    expectNoWarningState(host);
    expect(host.querySelector('[role="status"][aria-label="No applicable updates"]')?.textContent).toBe("No updates found");
    expect(host.querySelector('button[aria-label^="Update "]:not([aria-label="Update all"])')).toBeNull();
    expect(host.querySelector<HTMLButtonElement>('[aria-label="Update all"]')?.disabled).toBe(true);
    expect(host.querySelector('section[aria-label="Plugins updates"]')?.textContent).toContain("Found at last check");
    const details = host.querySelector<HTMLDetailsElement>('details[aria-label="Update check details"]')!;
    expect(details.open).toBe(false);
    expect(details.textContent).toContain(`official plugin channel: ${message}`);
    expect(fake.plugin).not.toHaveBeenCalled();
  } finally { await act(async () => root.unmount()); host.remove(); }
});

const feedDown = "Failed to fetch release: domios-dev/domios: manifest fetch returned HTTP 504";
const runningApp: UpdateAvailabilitySnapshot["items"][number] = { id: "app", kind: "app", installed: "1.19.0", latest: "1.19.0", compatible: true, state: "up_to_date", reason: "cached_check", channel: "official" };

async function renderPanel(snapshot: UpdateAvailabilitySnapshot) {
  fake.check.mockResolvedValue(snapshot);
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<UpdatePanelProvider><UpdatePanel /></UpdatePanelProvider>));
  return { host, dispose: async () => { await act(async () => root.unmount()); host.remove(); } };
}

it.each([
  ["a cached same-version check", runningApp, "Up to date at last check"],
  ["no check at all", { ...runningApp, latest: null, reason: "not_checked" }, "Not checked yet"],
])("shows the running version and its last known state, with no warning, when the app channel fails after %s", async (_case, item, lastKnown) => {
  const { host, dispose } = await renderPanel({ errors: { app: feedDown }, items: [item] });
  try {
    const row = host.querySelector('section[aria-label="App updates"]')!;
    expect(row.textContent).toContain("1.19.0");
    expect(row.textContent).toContain(lastKnown);
    expect(row.textContent).not.toContain("→");
    expect(host.textContent).not.toContain("Restart required");
    expectNoWarningState(host);
    expect(row.querySelector("[role=status]")).toBeNull();
    expect(host.querySelector('details[aria-label="Update check details"]')?.textContent).toContain("manifest fetch returned HTTP 504");
    expect(host.querySelector('button[aria-label="Update Domios"]')).toBeNull();
    expect(host.querySelector<HTMLButtonElement>('[aria-label="Update all"]')?.disabled).toBe(true);
  } finally { await dispose(); }
});

it("keeps a cached newer app update actionable without a notice about the source", async () => {
  const { host, dispose } = await renderPanel({ errors: { app: feedDown }, items: [{ ...runningApp, latest: "1.19.1", state: "update_available", reason: "cached_manifest" }] });
  try {
    const row = host.querySelector('section[aria-label="App updates"]')!;
    expect(row.textContent).toContain("1.19.1");
    expect(host.textContent).not.toContain("Restart required");
    expect(host.querySelector<HTMLButtonElement>('button[aria-label="Update Domios"]')?.disabled).toBe(false);
    expectNoWarningState(host);
    expect(host.querySelector('details[aria-label="Update check details"]')?.textContent).toContain("App update channel");
  } finally { await dispose(); }
});

it("names the path that served a healthy check only in the closed details", async () => {
  const { host, dispose } = await renderPanel({ errors: {}, served_by: { app: "github_api" }, items: [{ ...runningApp, reason: "same_version" }] });
  try {
    const details = host.querySelector<HTMLDetailsElement>('details[aria-label="Update check details"]')!;
    expect(details.open).toBe(false);
    expect(details.textContent).toContain("App update channel: served by GitHub API fallback");
    expect(host.querySelector('[role="status"][aria-label="No applicable updates"]')?.textContent).toBe("Everything is up to date");
  } finally { await dispose(); }
});

it.each([
  ["a rejected check", () => fake.check.mockRejectedValue(new Error("daemon unavailable"))],
  ["a check with a failed source", () => fake.check.mockResolvedValue({ errors: { app: feedDown }, items: [runningApp] })],
])("answers Check again after %s with one neutral line and no alarm", async (_case, arrange) => {
  const { host, dispose } = await renderPanel({ errors: {}, items: [] });
  try {
    arrange();
    await act(async () => [...host.querySelectorAll("button")].find(button => button.textContent === "Check again")!.click());
    expect(fake.check).toHaveBeenLastCalledWith(true);
    expect(host.querySelector('[role="status"][aria-label="Update check result"]')?.textContent).toBe("Couldn't check right now.");
    expectNoWarningState(host);
    expect(fake.app.applyUpdate).not.toHaveBeenCalled();
  } finally { await dispose(); }
});

it("asks for a restart only while a newer version is staged", async () => {
  const offer: UpdateAvailabilitySnapshot = { errors: {}, items: [{ ...runningApp, latest: "1.19.1", state: "update_available", reason: "newer_version" }] };
  const current: UpdateAvailabilitySnapshot = { errors: {}, items: [{ ...runningApp, reason: "same_version" }] };
  Object.assign(fake.app, { phase: "success" });
  const staged = await renderPanel(offer);
  try { expect(staged.host.textContent).toContain("Restart required"); } finally { await staged.dispose(); }
  const installed = await renderPanel(current);
  try { expect(installed.host.textContent).not.toContain("Restart required"); } finally { await installed.dispose(); }
  Object.assign(fake.app, { phase: "idle" });
  const offered = await renderPanel(offer);
  try { expect(offered.host.textContent).not.toContain("Restart required"); } finally { await offered.dispose(); }
});

it("clears a restored marker for the installed version at startup even when discovery never answers", async () => {
  localStorage.setItem("codeterm.update-all.intent", JSON.stringify({ stage: "app", appTarget: "1.19.0", pluginIds: [], interfaceRequested: false }));
  Object.assign(fake.app, { currentVersion: "1.19.0", nativeStatusObserved: true });
  fake.check.mockRejectedValue(new Error("daemon unavailable"));
  let panel!: ReturnType<typeof useUpdatePanel>;
  function Probe() { panel = useUpdatePanel(); return null; }
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<UpdatePanelProvider><Probe /></UpdatePanelProvider>));
    expect(localStorage.getItem("codeterm.update-all.intent")).toBeNull();
    expect(panel.busy).toBe(false);
    expect(panel.activeId).toBeNull();
    expect(panel.error).toBeFalsy();
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("keeps a restored marker with plugin work queued after the installed app", async () => {
  const saved = JSON.stringify({ stage: "app", appTarget: "1.19.0", pluginIds: ["git"], interfaceRequested: false });
  localStorage.setItem("codeterm.update-all.intent", saved);
  Object.assign(fake.app, { currentVersion: "1.19.0", nativeStatusObserved: true });
  fake.check.mockRejectedValue(new Error("daemon unavailable"));
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<UpdatePanelProvider><UpdatePanel /></UpdatePanelProvider>));
    expect(localStorage.getItem("codeterm.update-all.intent")).toBe(saved);
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it.each(["1.18.2", "1.18.3"])("silently clears a restored app target satisfied by host %s", async (installed) => {
  localStorage.setItem("codeterm.update-all.intent", JSON.stringify({ stage: "app", appTarget: "1.18.2", pluginIds: [], interfaceRequested: false }));
  Object.assign(fake.app, { currentVersion: installed, nativeStatusObserved: true });
  fake.check.mockResolvedValue({ errors: {}, items: [{ id: "app", kind: "app", installed, latest: installed, compatible: true, state: "up_to_date", reason: "same_version", channel: null }] });
  let panel!: ReturnType<typeof useUpdatePanel>;
  function Probe() { panel = useUpdatePanel(); return <UpdatePanel />; }
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<UpdatePanelProvider><Probe /></UpdatePanelProvider>));
    expect(localStorage.getItem("codeterm.update-all.intent")).toBeNull();
    expect(panel.error).toBeFalsy();
    expect(panel.history).toEqual([]);
    expect(panel.busy).toBe(false);
    expect(panel.awaitingRetry).toBe(false);
    expect(panel.activeId).toBeNull();
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(fake.app.applyUpdate).not.toHaveBeenCalled();
    expect(fake.plugin).not.toHaveBeenCalled();
    expect(fake.check).toHaveBeenCalledTimes(1);
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("reconciles only a restored app intent after native idle is observed without replay", async () => {
  const saved = JSON.stringify({ stage: "app", appTarget: "2.0.0", pluginIds: [], interfaceRequested: false });
  localStorage.setItem("codeterm.update-all.intent", saved);
  fake.check.mockResolvedValue({ errors: {}, items: [{ ...available.items[0], id: "app", kind: "app" }] });
  let panel!: ReturnType<typeof useUpdatePanel>;
  function Probe() { panel = useUpdatePanel(); return null; }
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const render = () => root.render(<UpdatePanelProvider><Probe /></UpdatePanelProvider>);
  try {
    await act(async () => render());
    expect(panel.error).toBeFalsy();
    Object.assign(fake.app, { nativeStatusObserved: true, phase: "downloading", installing: true });
    await act(async () => render());
    expect(panel.busy).toBe(true);
    expect(panel.error).toBeFalsy();
    Object.assign(fake.app, { phase: "idle", installing: false });
    await act(async () => render());
    expect(panel.busy).toBe(false);
    expect(panel.activeId).toBeNull();
    expect(panel.awaitingRetry).toBe(true);
    expect(panel.rows.find(row => row.id === "app")?.state).toBe("failed");
    expect(panel.error).toBeTruthy();
    expect(localStorage.getItem("codeterm.update-all.intent")).toBe(saved);
    expect(fake.app.applyUpdate).not.toHaveBeenCalled();
    await act(async () => panel.retry());
    expect(localStorage.getItem("codeterm.update-all.intent")).toBeNull();
    expect(fake.app.applyUpdate).not.toHaveBeenCalled();
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it.each([
  ["check", "all"], ["check", "item"], ["apply", "all"], ["apply", "item"],
])("retires a previous failure on %s before a new %s update applies and succeeds", async (retirement, action) => {
  localStorage.setItem("codeterm.update-all.intent", JSON.stringify({ stage: "app", appTarget: "2.0.0", pluginIds: [], interfaceRequested: false }));
  Object.assign(fake.app, { nativeStatusObserved: true });
  const offer: UpdateAvailabilitySnapshot = { errors: {}, items: [{ id: "app", kind: "app", installed: "1.0.0", latest: "2.0.0", compatible: true, state: "update_available", reason: "newer", channel: null }] };
  fake.check.mockResolvedValue(offer);
  fake.app.applyUpdate.mockImplementation(async () => {
    Object.assign(fake.app, { installing: true, phase: "applying" });
    return { status: "updating" };
  });
  let panel!: ReturnType<typeof useUpdatePanel>;
  function Probe() { panel = useUpdatePanel(); return null; }
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<UpdatePanelProvider><Probe /></UpdatePanelProvider>));
    expect(panel.error).toContain("previous app update is no longer active");
    expect(panel.awaitingRetry).toBe(true);
    fake.check.mockResolvedValue({ items: [], errors: {} });
    await act(async () => panel.retry());
    expect(panel.history).toHaveLength(1);
    expect(panel.error).toBeFalsy();
    fake.check.mockResolvedValue(offer);
    if (retirement === "check") {
      await act(async () => panel.retry());
      expect(panel.history).toEqual([]);
    } else {
      await act(async () => fake.changed!({ payload: offer }));
      expect(panel.history).toHaveLength(1);
    }
    await act(async () => action === "all" ? panel.updateAll() : panel.applyItem("app"));
    expect(fake.app.applyUpdate).toHaveBeenCalledExactlyOnceWith({ failureSurface: "inline" });
    expect(panel.busy).toBe(true);
    expect(panel.activeId).toBe("app");
    expect(panel.history).toEqual([]);
    expect(panel.error).toBeFalsy();
    Object.assign(fake.app, { installing: false, phase: "success", currentVersion: "2.0.0" });
    const installed: UpdateAvailabilitySnapshot = { errors: {}, items: offer.items.map(item => ({ ...item, installed: "2.0.0", state: "up_to_date" })) };
    fake.check.mockResolvedValue(installed);
    await act(async () => fake.changed!({ payload: installed }));
    expect(panel.busy).toBe(false);
    expect(panel.history).toEqual([]);
    expect(panel.error).toBeFalsy();
    expect(panel.awaitingRetry).toBe(false);
    expect(panel.rows.find(row => row.id === "app")?.state).toBe("completed");
  } finally { await act(async () => root.unmount()); }
});

it.each(["applying", "success"])("retires archived failures when the native updater reports %s", async phase => {
  localStorage.setItem("codeterm.update-all.intent", JSON.stringify({ stage: "app", appTarget: "2.0.0", pluginIds: [], interfaceRequested: false }));
  Object.assign(fake.app, { nativeStatusObserved: true });
  fake.check.mockResolvedValue({ items: [], errors: {} });
  let panel!: ReturnType<typeof useUpdatePanel>;
  function Probe() { panel = useUpdatePanel(); return null; }
  const host = document.createElement("div");
  const root = createRoot(host);
  const render = () => root.render(<UpdatePanelProvider><Probe /></UpdatePanelProvider>);
  try {
    await act(async () => render());
    expect(panel.error).toBeTruthy();
    await act(async () => panel.retry());
    expect(panel.history).toHaveLength(1);
    Object.assign(fake.app, { phase, installing: phase === "applying" });
    await act(async () => render());
    expect(panel.history).toEqual([]);
    Object.assign(fake.app, { phase: "idle", installing: false });
    await act(async () => render());
    expect(panel.history).toEqual([]);
  } finally { await act(async () => root.unmount()); }
});
