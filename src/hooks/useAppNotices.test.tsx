import { act, useCallback, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Notification } from "../lib/types";
import type { BuildMismatch } from "../lib/buildMismatchGuard";
import type { FrontendUpdateOffer } from "../lib/frontendUpdate/runtime";

const fixture = vi.hoisted(() => ({
  mismatch: null as ((value: BuildMismatch | null) => void) | null,
  initialMismatch: null as BuildMismatch | null,
  offer: null as FrontendUpdateOffer | null,
  offerListeners: new Set<() => void>(),
  reload: vi.fn(async (_hash?: string) => {}),
  accept: vi.fn(),
  actions: null as { upsertToast: (toast: Omit<Notification, "timestamp" | "read">) => void; dismissToast: (id: string) => void } | null,
}));
vi.mock("../store/multiplexer", () => ({ useMultiplexer: () => fixture.actions }));
vi.mock("../lib/buildMismatchGuard", () => ({
  onMismatch: (listener: typeof fixture.mismatch) => { fixture.mismatch = listener; return () => { fixture.mismatch = null; }; },
  checkBuildMismatch: async () => fixture.initialMismatch, installBuildDriftTriggers: () => () => {},
}));
vi.mock("../lib/serviceWorkerRegistration", () => ({ reloadInterface: fixture.reload, registerServiceWorker: vi.fn() }));
vi.mock("../lib/frontendUpdate/browser", () => ({
  frontendUpdateOffer: () => fixture.offer,
  subscribeFrontendUpdateOffer: (listener: () => void) => { fixture.offerListeners.add(listener); return () => fixture.offerListeners.delete(listener); },
  acceptFrontendUpdate: fixture.accept,
}));
vi.mock("../lib/webApi", async (importOriginal) => ({ ...await importOriginal<typeof import("../lib/webApi")>(), IS_WEB: true, GATEWAY_STATE_EVENT: "gateway-state", tokenFor: () => null, selfHost: () => "localhost" }));

import { useAppNotices } from "./useAppNotices";
import { NotificationToastContainer } from "../components/Notifications/NotificationToast";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Notices() { useAppNotices(); return null; }
function Harness() {
  const [toasts, setToasts] = useState<Notification[]>([]);
  const upsertToast = useCallback((toast: Omit<Notification, "timestamp" | "read">) => setToasts((current) => [...current.filter((item) => item.id !== toast.id), { ...toast, timestamp: 1, read: false }]), []);
  const dismissToast = useCallback((id: string) => setToasts((current) => current.filter((item) => item.id !== id)), []);
  fixture.actions = { upsertToast, dismissToast };
  return <><Notices /><NotificationToastContainer toasts={toasts} onDismiss={dismissToast} /></>;
}

let host: HTMLDivElement;
let root: Root;
const toast = (id: string) => host.querySelector<HTMLElement>(`[data-notification-id="${id}"]`);
const action = (id: string, label: string) => [...toast(id)!.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === label)!;
beforeEach(async () => {
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  Object.defineProperty(navigator, "onLine", { value: true, configurable: true });
  localStorage.clear();
  await act(async () => root.render(<Harness />));
});
afterEach(async () => {
  await act(async () => root.unmount()); host.remove();
  fixture.offer = null; fixture.initialMismatch = null; fixture.reload.mockReset(); fixture.accept.mockReset();
});

it("puts build drift and reload failure in the stack with Reload, Copy details and Retry", async () => {
  await act(async () => fixture.mismatch!({ reason: "build", frontendHash: "old", backendHash: "new", frontendVersion: "old", backendVersion: "new", apiVersion: 1, minApiVersion: 1, maxApiVersion: 1 }));
  expect(toast("interface-build-mismatch")).not.toBeNull();
  expect(host.querySelector('[data-testid="stale-build-banner"]')).toBeNull();
  expect(action("interface-build-mismatch", "Copy details")).toBeDefined();
  fixture.reload.mockRejectedValueOnce(new Error("opaque reload error")).mockRejectedValueOnce(new Error("retry failure"));
  await act(async () => action("interface-build-mismatch", "Reload interface").click());
  expect(fixture.reload).toHaveBeenCalledWith("new");
  expect(toast("interface-reload-failure")).not.toBeNull();
  await act(async () => action("interface-reload-failure", "Retry").click());
  expect(fixture.reload).toHaveBeenCalledTimes(2);
  expect(toast("interface-reload-failure")).not.toBeNull();
  await act(async () => action("interface-reload-failure", "Retry").click());
  expect(fixture.reload).toHaveBeenCalledTimes(3);
});

it("shows an already detected build mismatch once and copies its details", async () => {
  await act(async () => root.unmount());
  fixture.initialMismatch = { reason: "build", frontendHash: "old", backendHash: "new", frontendVersion: "old", backendVersion: "new", apiVersion: 1, minApiVersion: 1, maxApiVersion: 1 };
  root = createRoot(host);
  const copy = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
  try {
    await act(async () => root.render(<Harness />));
    await act(async () => action("interface-build-mismatch", "Copy details").click());
    expect(copy).toHaveBeenCalledWith("Interface build: old; daemon build: new");
    await act(async () => fixture.mismatch!(fixture.initialMismatch));
    expect(toast("interface-build-mismatch")).toBeNull();
  } finally { copy.mockRestore(); }
});

it("offers the frontend update through the stack and runs its action", async () => {
  await act(async () => { fixture.offer = { releaseId: "release", version: "next" }; for (const listener of fixture.offerListeners) listener(); });
  expect(toast("interface-update-available")).not.toBeNull();
  await act(async () => action("interface-update-available", "Update").click());
  expect(fixture.accept).toHaveBeenCalledOnce();
});

it("shows offline and gateway state in the stack and removes them on recovery", async () => {
  await act(async () => { Object.defineProperty(navigator, "onLine", { value: false, configurable: true }); window.dispatchEvent(new Event("offline")); });
  expect(toast("offline")).not.toBeNull();
  expect(host.querySelector('[data-testid="offline-indicator"]')).toBeNull();
  await act(async () => window.dispatchEvent(new Event("codeterm-cache-hydrated")));
  expect(toast("offline")?.textContent).toContain("saved state");
  await act(async () => window.dispatchEvent(new CustomEvent("gateway-state", { detail: { available: false } })));
  expect(toast("gateway-unavailable")).not.toBeNull();
  await act(async () => {
    Object.defineProperty(navigator, "onLine", { value: true, configurable: true }); window.dispatchEvent(new Event("online"));
    window.dispatchEvent(new CustomEvent("gateway-state", { detail: { available: true } }));
  });
  expect(toast("offline")).toBeNull(); expect(toast("gateway-unavailable")).toBeNull();
});

it("puts install prompts in the stack with the browser Install action", async () => {
  const prompt = vi.fn(async () => {});
  const event = new Event("beforeinstallprompt", { cancelable: true });
  Object.assign(event, { prompt, userChoice: Promise.resolve({ outcome: "accepted" }) });
  await act(async () => window.dispatchEvent(event));
  expect(event.defaultPrevented).toBe(true);
  expect(host.querySelector('[data-testid="pwa-chrome-install-hint"]')).toBeNull();
  await act(async () => action("pwa-install", "Install").click());
  expect(prompt).toHaveBeenCalledOnce();
});

it("puts iOS installation instructions in the stack with a persisted opt-out", async () => {
  await act(async () => root.unmount());
  const userAgent = navigator.userAgent;
  Object.defineProperty(navigator, "userAgent", { value: "iPhone", configurable: true });
  try {
    root = createRoot(host);
    await act(async () => root.render(<Harness />));
    expect(toast("pwa-install-ios")).not.toBeNull();
    expect(host.querySelector('[data-testid="pwa-install-hint"]')).toBeNull();
    await act(async () => action("pwa-install-ios", "Do not show again").click());
    expect(localStorage.getItem("codeterm-pwa-hint-dismissed")).toBe("1");
  } finally { Object.defineProperty(navigator, "userAgent", { value: userAgent, configurable: true }); }
});
