import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { BuildMismatch } from "../lib/buildMismatchGuard";
import type { Notification } from "../lib/types";
import { MultiplexerActionsContext, type MultiplexerActionsContextType } from "../store/multiplexer/context";
import { NotificationToastContainer } from "./Notifications/NotificationToast";
import { StaleBuildBanner } from "./StaleBuildBanner";

const fakes = vi.hoisted(() => ({
  listener: null as ((m: BuildMismatch | null) => void) | null,
  uninstall: vi.fn(),
  reload: vi.fn(async () => {}),
}));
vi.mock("../lib/serviceWorkerRegistration", () => ({ reloadInterface: fakes.reload }));
vi.mock("../lib/buildMismatchGuard", () => ({
  checkBuildMismatch: vi.fn(async () => null),
  installBuildDriftTriggers: vi.fn(() => fakes.uninstall),
  onMismatch: (fn: (m: BuildMismatch | null) => void) => { fakes.listener = fn; return () => { fakes.listener = null; }; },
}));

const mismatch = (reason: BuildMismatch["reason"]): BuildMismatch => ({
  frontendHash: "old", backendHash: "new", frontendVersion: "1", backendVersion: "1",
  reason, apiVersion: 3, minApiVersion: 3, maxApiVersion: 3,
});

function Harness() {
  const [toasts, setToasts] = useState<Notification[]>([]);
  const actions = {
    upsertToast: (toast: Omit<Notification, "timestamp" | "read">) => setToasts((current) => [
      ...current.filter((item) => item.id !== toast.id), { ...toast, timestamp: 1, read: false },
    ]),
    dismissToast: (id: string) => setToasts((current) => current.filter((toast) => toast.id !== id)),
  } as unknown as MultiplexerActionsContextType;
  return <MultiplexerActionsContext.Provider value={actions}>
    <StaleBuildBanner />
    <NotificationToastContainer toasts={toasts} onDismiss={(id) => setToasts((current) => current.filter((toast) => toast.id !== id))} />
  </MultiplexerActionsContext.Provider>;
}

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  fakes.reload.mockReset().mockResolvedValue(undefined);
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

test.each(["build", "build-unreachable"] as const)("shows the %s mismatch as a toast with a reload action", async (reason) => {
  await act(async () => root.render(<Harness />));
  await act(async () => fakes.listener?.(mismatch(reason)));
  expect(host.querySelector('[data-testid="notification-toast"]')?.getAttribute("role")).toBe("status");
  expect(host.querySelector('[role="alert"]')).toBeNull();
  const reloadAction = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent === "Reload interface");
  expect(reloadAction).toBeDefined();
  await act(async () => reloadAction!.click());
  expect(fakes.reload).toHaveBeenLastCalledWith("new");
  await act(async () => root.unmount());
  expect(fakes.uninstall).toHaveBeenCalled();
  root = createRoot(host);
});

test("dismissal stays cleared across a rerender", async () => {
  await act(async () => root.render(<Harness />));
  await act(async () => fakes.listener?.(mismatch("build")));
  expect(Array.from(host.querySelectorAll("button")).some((button) => button.textContent === "Copy details")).toBe(true);
  await act(async () => fakes.listener?.(null));
  expect(host.querySelector('[data-testid="notification-toast"]')).toBeNull();
  await act(async () => fakes.listener?.(mismatch("build")));
  await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Dismiss"]')!.click());
  await act(async () => root.render(<Harness />));
  expect(host.querySelector('[data-testid="notification-toast"]')).toBeNull();
});

test("a failed interface reload is retryable from a toast", async () => {
  fakes.reload.mockRejectedValueOnce(new Error("offline"));
  await act(async () => root.render(<Harness />));
  await act(async () => fakes.listener?.(mismatch("build")));
  const reload = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent === "Reload interface")!;
  await act(async () => reload.click());
  expect(host.querySelector('[role="alert"]')).toBeNull();
  const retry = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent === "Retry");
  expect(retry).toBeDefined();
  await act(async () => retry!.click());
  expect(fakes.reload).toHaveBeenCalledTimes(2);
});
