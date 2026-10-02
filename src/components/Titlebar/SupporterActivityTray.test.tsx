import { act } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { API, harness, statusFixture } from "../Supporter/testSupport";
import { SupporterActivityTray } from "./SupporterActivityTray";

vi.mock("../../lib/webApi", async (importOriginal) => ({
  ...await importOriginal<typeof import("../../lib/webApi")>(),
  daemonFetch: (_api: unknown, path: string, init: RequestInit) => fetch(path, init),
  daemonAuthToken: () => "fixture",
}));
let setup: ReturnType<typeof harness>;
beforeEach(() => {
  setup = harness();
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => new Response(JSON.stringify(String(input).includes("/activity?") ? { entries: [{ time: 1700000000, team_id: null, kind: "tab_state", label: "other", confidence_band: "strong", action: "notify_only", reason: "launch_ready", budget_reached: false, fallback: false }] } : statusFixture()), { status: 200 }));
});
afterEach(async () => setup.cleanup());

describe("General Supporter entry", () => {
  it("opens_the_machine_feed_outside_titlebar_clipping_and_restores_focus_on_escape", async () => {
    const host = await setup.render(<SupporterActivityTray api={API} />);
    const button = host.querySelector<HTMLButtonElement>('button[aria-haspopup="dialog"]')!;
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    const closedListeners = setup.listeners.size;
    await act(async () => button.click());
    const dialog = document.querySelector('[role="dialog"]');
    expect(dialog?.parentElement).toBe(document.body);
    expect(dialog?.querySelectorAll("ol > li")).toHaveLength(1);
    expect(dialog?.querySelectorAll("[data-estimate]")).toHaveLength(3);
    expect(button.getAttribute("aria-expanded")).toBe("true");
    expect(button.getAttribute("aria-controls")).toBe(dialog?.id);
    await act(async () => document.dispatchEvent(new KeyboardEvent("keydown", { code: "Escape" })));
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(button);
    expect(setup.listeners.size).toBe(closedListeners);
  });
  it("stays_hidden_until_the_supporter_is_connected", async () => {
    vi.mocked(fetch).mockImplementation(async () => new Response(JSON.stringify({ ...statusFixture(), connected: false }), { status: 200 }));
    const host = await setup.render(<SupporterActivityTray api={API} />);
    expect(host.querySelector('button[aria-haspopup="dialog"]')).toBeNull();
  });
});
