import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { UpdateAvailabilitySnapshot } from "../lib/updateAvailability";

const fake = vi.hoisted(() => ({ check: vi.fn(), plugin: vi.fn(), changed: null as ((event: { payload: UpdateAvailabilitySnapshot }) => void) | null }));
vi.mock("../lib/updateAvailability", () => ({ checkUpdateAvailability: fake.check }));
vi.mock("../hooks/useUpdate", () => ({ useUpdate: () => ({ installing: false, error: null, currentVersion: "1.0.0" }) }));
vi.mock("../lib/pluginSnapshotStore", () => ({ runPluginSnapshotAction: fake.plugin }));
vi.mock("../lib/frontendUpdate/browser", () => ({ acceptFrontendUpdate: vi.fn(), setFrontendUpdateHeld: vi.fn(), frontendUpdateOffer: () => null, subscribeFrontendUpdateOffer: () => () => {} }));
vi.mock("../lib/bridge", () => ({ listen: async (_event: string, handler: typeof fake.changed) => { fake.changed = handler; return () => {}; } }));
import { UpdatePanelProvider } from "../hooks/useUpdatePanel";
import { UpdatePanel } from "./UpdatePanel";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const available: UpdateAvailabilitySnapshot = { errors: {}, items: ["git", "grok"].map(id => ({ id, kind: "plugin", installed: "1.0.0", latest: "2.0.0", compatible: true, state: "update_available", reason: "newer", channel: "official" })) };
afterEach(() => { localStorage.clear(); vi.resetAllMocks(); });

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
      expect(host.querySelector('button[aria-label^="Apply"]:not(:disabled)')).toBeNull();
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
