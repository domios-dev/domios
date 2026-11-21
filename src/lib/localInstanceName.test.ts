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
  expect(rememberLocalInstanceName("BLACKSTAR [dev]")).toBe("BLACKSTAR");
  expect(rememberLocalInstanceName("")).toBe("BLACKSTAR");
  expect(rememberLocalInstanceName(undefined)).toBe("BLACKSTAR");
  expect(rememberLocalInstanceName("  ")).toBe("BLACKSTAR");
  expect(localInstanceLabel()).toBe("BLACKSTAR");
});

it("delivers a mesh-peers-loaded signal until the listener is released", () => {
  const handler = vi.fn();
  const stop = onMeshPeersLoaded(handler);
  notifyMeshPeersLoaded();
  stop();
  notifyMeshPeersLoaded();
  expect(handler).toHaveBeenCalledTimes(1);
});
