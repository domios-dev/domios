import { afterEach, expect, it, vi } from "vitest";
import {
  __resetLocalInstanceNameForTest,
  localInstanceLabel,
  notifyMeshPeersLoaded,
  onMeshPeersLoaded,
  rememberLocalInstanceName,
} from "./localInstanceName";

afterEach(() => __resetLocalInstanceNameForTest());

it("keeps the last reported name and ignores empty reports", () => {
  expect(localInstanceLabel()).toBe("Domios");
  expect(rememberLocalInstanceName("BLACKSTAR [dev]")).toBe("BLACKSTAR [dev]");
  expect(rememberLocalInstanceName("")).toBe("BLACKSTAR [dev]");
  expect(rememberLocalInstanceName(undefined)).toBe("BLACKSTAR [dev]");
  expect(rememberLocalInstanceName("  ")).toBe("BLACKSTAR [dev]");
  expect(localInstanceLabel()).toBe("BLACKSTAR [dev]");
});

it("delivers a mesh-peers-loaded signal until the listener is released", () => {
  const handler = vi.fn();
  const stop = onMeshPeersLoaded(handler);
  notifyMeshPeersLoaded();
  stop();
  notifyMeshPeersLoaded();
  expect(handler).toHaveBeenCalledTimes(1);
});
