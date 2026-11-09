import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const settings = vi.hoisted(() => ({ stored: undefined as unknown, setUiSettings: vi.fn(async (_patch: Record<string, unknown>) => {}) }));
vi.mock("../../lib/clientSettings", () => ({
  loadConfig: async () => ({ ui: { rate_us: settings.stored } }),
  setUiSettings: settings.setUiSettings,
}));
const bridge = vi.hoisted(() => ({ openUrl: vi.fn(async (_url: string) => {}) }));
vi.mock("../../lib/nativeBridge", () => ({ nativeBridge: bridge }));

import { siteLinks } from "../../../shared/siteLinks";
import { RateUsTray } from "./RateUsTray";
import { DAY_MS, SNOOZE_MS, localDayKey, type RateUsState } from "./rateUsModel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const NOW = Date.UTC(2026, 10, 5, 12, 0);
const DUE: RateUsState = { first_seen: NOW - 2 * DAY_MS, days: [localDayKey(NOW - 2 * DAY_MS), localDayKey(NOW)], snoozes: 0, snooze_until: 0, done: false };
let root: Root | null = null;
let host: HTMLDivElement;

beforeEach(() => {
  settings.stored = DUE;
  settings.setUiSettings.mockClear();
  bridge.openUrl.mockClear();
});

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

async function mount(locked = false) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<RateUsTray locked={locked} now={() => NOW} />));
  await act(async () => {});
}

const invite = () => document.body.querySelector("[data-testid=rate-us-invite]");
const button = (name: string) => Array.from(document.body.querySelectorAll("button")).find((item) => item.textContent?.trim() === name)!;
const lastSaved = () => settings.setUiSettings.mock.calls[settings.setUiSettings.mock.calls.length - 1]?.[0].rate_us as RateUsState;

it("shows the invite when due without taking focus, and Star opens the repository and ends it", async () => {
  const outside = document.createElement("button");
  document.body.append(outside);
  outside.focus();
  await mount();
  expect(invite()).not.toBeNull();
  expect(document.activeElement).toBe(outside);
  await act(async () => button("Star on GitHub").click());
  expect(bridge.openUrl).toHaveBeenCalledExactlyOnceWith(siteLinks.repository);
  expect(lastSaved().done).toBe(true);
  expect(invite()).toBeNull();
});

it("Later snoozes for seven days and hides it", async () => {
  await mount();
  await act(async () => button("Later").click());
  expect(lastSaved()).toMatchObject({ snoozes: 1, snooze_until: NOW + SNOOZE_MS, done: false });
  expect(invite()).toBeNull();
  expect(bridge.openUrl).not.toHaveBeenCalled();
});

it("Don't ask again ends it without opening anything", async () => {
  await mount();
  await act(async () => button("Don’t ask again").click());
  expect(lastSaved().done).toBe(true);
  expect(invite()).toBeNull();
  expect(bridge.openUrl).not.toHaveBeenCalled();
});

it("the tray star folds the dropdown away and back without answering it", async () => {
  await mount();
  const star = host.querySelector<HTMLButtonElement>("button[aria-controls]")!;
  await act(async () => star.click());
  expect(invite()).toBeNull();
  expect(settings.setUiSettings).not.toHaveBeenCalled();
  await act(async () => star.click());
  expect(invite()).not.toBeNull();
  await act(async () => button("Later").focus());
  await act(async () => button("Later").dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
  expect(invite()).toBeNull();
  expect(document.activeElement).toBe(star);
  expect(settings.setUiSettings).not.toHaveBeenCalled();
});

it("stays away during first run and while a modal is open", async () => {
  await mount(true);
  expect(host.childElementCount).toBe(0);
  await act(async () => root!.unmount());
  const modal = document.createElement("div");
  modal.setAttribute("aria-modal", "true");
  document.body.append(modal);
  await mount();
  expect(invite()).toBeNull();
  modal.remove();
  await act(async () => window.dispatchEvent(new Event("pointerdown")));
  expect(invite()).not.toBeNull();
});

it("a new user is only recorded, never invited", async () => {
  settings.stored = undefined;
  await mount();
  expect(host.childElementCount).toBe(0);
  expect(lastSaved()).toEqual({ first_seen: NOW, days: [localDayKey(NOW)], snoozes: 0, snooze_until: 0, done: false });
});
