import { afterEach, describe, expect, it, vi } from "vitest";
import {
  defaultMachineGroupExpanded,
  isMachineGroupExpanded,
  loadMachineGroupPrefs,
  MAX_MACHINE_GROUP_PREFS,
  persistMachineGroupPrefs,
  sanitizeMachineGroupPrefs,
  setMachineGroupExpanded,
  SIDEBAR_MACHINE_GROUPS_KEY,
} from "./workspaceMachineGroupsModel";

afterEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
});

describe("workspaceMachineGroupsModel", () => {
  it("expands all machines when there are 1 or 2, and collapses non-local machines when there are more than 2", () => {
    expect(defaultMachineGroupExpanded(null, 1)).toBe(true);
    expect(defaultMachineGroupExpanded(null, 2)).toBe(true);
    expect(defaultMachineGroupExpanded("rog", 2)).toBe(true);

    expect(defaultMachineGroupExpanded(null, 3)).toBe(true);
    expect(defaultMachineGroupExpanded("rog", 3)).toBe(false);
    expect(defaultMachineGroupExpanded(null, 6)).toBe(true);
    for (const peer of ["rog", "ubunta", "eight", "rollacodebot", "air"]) {
      expect(defaultMachineGroupExpanded(peer, 6)).toBe(false);
    }
  });

  it("prefers saved per-machine viewer state over defaults", () => {
    const prefs = { local: false, rog: true };
    expect(isMachineGroupExpanded(null, 6, prefs)).toBe(false);
    expect(isMachineGroupExpanded("rog", 6, prefs)).toBe(true);
    expect(isMachineGroupExpanded("air", 6, prefs)).toBe(false);
    expect(isMachineGroupExpanded("air", 2, { air: false })).toBe(false);
  });

  it("bounds persisted entries and refreshes recency on updates", () => {
    let prefs = {};
    prefs = setMachineGroupExpanded(prefs, "m1", true, 3);
    prefs = setMachineGroupExpanded(prefs, "m2", false, 3);
    prefs = setMachineGroupExpanded(prefs, "m3", true, 3);
    prefs = setMachineGroupExpanded(prefs, "m1", false, 3);
    prefs = setMachineGroupExpanded(prefs, "m4", true, 3);
    expect(Object.keys(prefs)).toEqual(["m3", "m1", "m4"]);
    expect(prefs).toEqual({ m3: true, m1: false, m4: true });

    const oversized: Record<string, boolean> = {};
    for (let index = 0; index < MAX_MACHINE_GROUP_PREFS + 10; index += 1) {
      oversized[`peer-${index}`] = index % 2 === 0;
    }
    const sanitized = sanitizeMachineGroupPrefs(oversized);
    expect(Object.keys(sanitized)).toHaveLength(MAX_MACHINE_GROUP_PREFS);
    expect(sanitized["peer-0"]).toBeUndefined();
    expect(sanitized[`peer-${MAX_MACHINE_GROUP_PREFS + 9}`]).toBe(false);
  });

  it("persists to localStorage, sanitizes invalid payloads, and tolerates storage failures", () => {
    persistMachineGroupPrefs({ local: true, rog: true, ubunta: false });
    expect(loadMachineGroupPrefs()).toEqual({ local: true, rog: true, ubunta: false });

    localStorage.setItem(SIDEBAR_MACHINE_GROUPS_KEY, JSON.stringify({ rog: true, bad: "yes", "": true, num: 1 }));
    expect(loadMachineGroupPrefs()).toEqual({ rog: true });

    localStorage.setItem(SIDEBAR_MACHINE_GROUPS_KEY, "{not-json");
    expect(loadMachineGroupPrefs()).toEqual({});

    localStorage.setItem(SIDEBAR_MACHINE_GROUPS_KEY, JSON.stringify([true, false]));
    expect(loadMachineGroupPrefs()).toEqual({});

    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("storage disabled");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota exceeded");
    });
    expect(loadMachineGroupPrefs()).toEqual({});
    expect(() => persistMachineGroupPrefs({ rog: true })).not.toThrow();
  });
});
