import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { BuildMismatch } from "../lib/buildMismatchGuard";
import { StaleBuildBanner } from "./StaleBuildBanner";

const fakes = vi.hoisted(() => ({
  listener: null as ((m: BuildMismatch | null) => void) | null,
  uninstall: vi.fn(),
}));
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
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

test.each(["build", "build-unreachable"] as const)("renders the %s reason with the reload action", reason => {
  act(() => root.render(<StaleBuildBanner />));
  expect(host.querySelector("[data-testid=stale-build-banner]")).toBeNull();
  act(() => fakes.listener?.(mismatch(reason)));
  expect(host.querySelector("[data-testid=stale-build-banner]")).not.toBeNull();
  const reload = [...host.querySelectorAll("button")].find(b => b.textContent === "Reload interface");
  expect(reload).toBeDefined();
  act(() => root.unmount());
  expect(fakes.uninstall).toHaveBeenCalled();
  root = createRoot(host);
});
