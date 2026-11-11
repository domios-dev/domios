import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { KeepAwakeStatus } from "../../../gen/KeepAwakeStatus";

const backend = vi.hoisted(() => ({
  invoke: vi.fn(),
  save: vi.fn(),
  listener: undefined as undefined | ((event: { payload: KeepAwakeStatus }) => void),
}));
vi.mock("../../lib/bridge", () => ({
  invoke: backend.invoke,
  listen: async (_name: string, handler: typeof backend.listener) => {
    backend.listener = handler;
    return () => { backend.listener = undefined; };
  },
}));
vi.mock("../../lib/clientSettings", () => ({
  getCachedConfig: () => null,
  loadConfig: async () => ({ ui: { keepawake_display: false } }),
  setUiSettings: backend.save,
}));
vi.mock("../shared/AnchoredPopover", () => ({
  AnchoredPopover: ({ open, children }: { open: boolean; children: React.ReactNode }) => open ? children : null,
}));
import { KeepAwakeTray } from "./KeepAwakeTray";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const off: KeepAwakeStatus = { active: false, display: false, expires_at: null, gnome_policy: false, system_block: false };
let state = off;
let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;

beforeEach(async () => {
  state = off;
  backend.invoke.mockReset();
  backend.save.mockReset().mockResolvedValue(undefined);
  backend.invoke.mockImplementation(async (command: string, args?: { durationSecs?: number; display?: boolean }) => {
    if (command === "keepawake_start") state = { ...off, active: true, display: args?.display ?? false, expires_at: args?.durationSecs ? Date.now() + args.durationSecs * 1000 : null };
    if (command === "keepawake_stop") { state = off; return; }
    return state;
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<KeepAwakeTray />));
});
afterEach(async () => { await act(async () => root.unmount()); host.remove(); });

const trigger = () => host.querySelector<HTMLButtonElement>('[data-testid="keepawake-tray"]')!;
async function click(selector: string) {
  const button = host.querySelector<HTMLButtonElement>(selector);
  expect(button).not.toBeNull();
  await act(async () => button!.click());
}
async function open() { await act(async () => trigger().click()); }

it("applies preset, display and stop results even when no change event arrives", async () => {
  await open();
  await click('[role="tab"][data-value="1h"]');
  expect(trigger().getAttribute("aria-pressed")).toBe("true");
  await click('[role="switch"]');
  expect(state.display).toBe(true);
  expect(host.querySelector('[role="switch"]')?.getAttribute("aria-checked")).toBe("true");
  await click('[aria-label="Turn keep awake off"]');
  expect(trigger().getAttribute("aria-pressed")).toBe("false");
});

it.each(["preset", "off", "display"])("shows a refused %s press and reconciles backend state", async control => {
  await open();
  await click('[role="tab"][data-value="1h"]');
  const normal = backend.invoke.getMockImplementation()!;
  backend.invoke.mockImplementation(async (command, args) => {
    if (command !== "keepawake_status") {
      state = off;
      throw new Error("opaque-inhibitor-refusal");
    }
    return normal(command, args);
  });
  await click(control === "preset" ? '[role="tab"][data-value="2h"]'
    : control === "off" ? '[aria-label="Turn keep awake off"]' : '[role="switch"]');
  expect(host.querySelector('[role="alert"]')?.textContent).toContain("opaque-inhibitor-refusal");
  expect(trigger().getAttribute("aria-pressed")).toBe("false");
  expect(backend.save).not.toHaveBeenCalled();
});

it("refreshes missed external starts and stops on each opening", async () => {
  state = { ...off, active: true, display: true };
  await open();
  expect(trigger().getAttribute("aria-pressed")).toBe("true");
  expect(host.querySelector('[role="switch"]')?.getAttribute("aria-checked")).toBe("true");
  await open();
  state = off;
  await open();
  expect(trigger().getAttribute("aria-pressed")).toBe("false");
});

it("does not let a late opening snapshot overwrite a newer event", async () => {
  let resolve: (value: KeepAwakeStatus) => void = () => {};
  backend.invoke.mockImplementationOnce(() => new Promise<KeepAwakeStatus>(done => { resolve = done; }));
  await open();
  await act(async () => backend.listener?.({ payload: { ...off, active: true } }));
  await act(async () => resolve(off));
  expect(trigger().getAttribute("aria-pressed")).toBe("true");
});

it("shows a refused inactive display preference without changing the switch", async () => {
  await open();
  backend.save.mockRejectedValueOnce(new Error("opaque-settings-refusal"));
  await click('[role="switch"]');
  expect(host.querySelector('[role="alert"]')?.textContent).toContain("opaque-settings-refusal");
  expect(host.querySelector('[role="switch"]')?.getAttribute("aria-checked")).toBe("false");
});
