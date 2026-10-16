import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { BuildMismatch } from "../lib/buildMismatchGuard";
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

test.each(["build", "build-unreachable"] as const)("renders the %s reason with the reload action", async reason => {
  act(() => root.render(<StaleBuildBanner />));
  expect(host.querySelector("[data-testid=stale-build-banner]")).toBeNull();
  act(() => fakes.listener?.(mismatch(reason)));
  expect(host.querySelector("[data-testid=stale-build-banner]")).not.toBeNull();
  const reload = host.querySelector<HTMLButtonElement>('button[aria-label="Reload interface"]');
  expect(reload).not.toBeNull();
  await act(async () => reload?.click());
  expect(fakes.reload).toHaveBeenLastCalledWith("new");
  act(() => root.unmount());
  expect(fakes.uninstall).toHaveBeenCalled();
  root = createRoot(host);
});

test("details and dismissal are explicit actions", () => {
  act(() => root.render(<StaleBuildBanner />));
  act(() => fakes.listener?.(mismatch("build")));
  expect(host.querySelector('[aria-label="Interface build details"]')).toBeNull();
  act(() => host.querySelector<HTMLButtonElement>('button[aria-label="Interface update details"]')?.click());
  expect(host.querySelector('[aria-label="Interface build details"]')).not.toBeNull();
  act(() => host.querySelector<HTMLButtonElement>('button[aria-label="Dismiss interface update"]')?.click());
  expect(host.querySelector('[aria-label="Interface update"]')).toBeNull();
});

test("a failed worker update remains retryable", async () => {
  fakes.reload.mockRejectedValueOnce(new Error("offline"));
  act(() => root.render(<StaleBuildBanner />));
  act(() => fakes.listener?.(mismatch("build")));
  const button = host.querySelector<HTMLButtonElement>('button[aria-label="Reload interface"]')!;
  await act(async () => button.click());
  expect(host.querySelector('[role="alert"]')).not.toBeNull();
  expect(button.disabled).toBe(false);
  await act(async () => button.click());
  expect(fakes.reload).toHaveBeenCalledTimes(2);
});
