import { afterEach, expect, it, vi } from "vitest";
import type { PluginSnapshot } from "../components/Extensions/types";

vi.mock("./bridge", () => ({ invoke: vi.fn() }));
import { invoke } from "./bridge";
import { __resetPluginSnapshotStoreForTest, ensurePluginSnapshot, runPluginSnapshotAction } from "./pluginSnapshotStore";

afterEach(() => {
  __resetPluginSnapshotStoreForTest();
  vi.resetAllMocks();
});

it.each(["success", "failure"])("an earlier snapshot %s cannot replace a completed update action", async (outcome) => {
  __resetPluginSnapshotStoreForTest();
  let resolve!: (snapshot: PluginSnapshot) => void;
  let reject!: (error: Error) => void;
  const oldRead = new Promise<PluginSnapshot>((yes, no) => { resolve = yes; reject = no; });
  const stale: PluginSnapshot = {
    plugins: [{ id: "demo", version: "0.1.0", update: { available: true, reason: "newer_version" } }],
    channels: [], diff: [], marketplace: [],
  };
  const current: PluginSnapshot = {
    ...stale, plugins: [{ id: "demo", version: "0.2.0", update: { available: false, reason: "same_version" } }],
  };
  vi.mocked(invoke).mockImplementation(async <T>(command: string): Promise<T> => {
    if (command === "plugin_manager_snapshot") return await oldRead as T;
    if (command === "plugin_manager_action") return current as T;
    throw new Error(`Unexpected command: ${command}`);
  });
  const pending = ensurePluginSnapshot({ force: true });
  await runPluginSnapshotAction({ kind: "update", pluginId: "demo" });
  if (outcome === "success") resolve(stale);
  else reject(new Error("superseded read failed"));
  expect(await pending).toEqual(current);
  expect(await ensurePluginSnapshot()).toEqual(current);
  expect(vi.mocked(invoke)).toHaveBeenCalledTimes(2);
});
