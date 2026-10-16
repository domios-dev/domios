import { describe, expect, it } from "vitest";
import {
  BUILD_DRIFT_RECORD_KEY, decideBuildDrift, isSignedReleasePage, persistDriftRecord, readDriftRecord,
  type DaemonIdentity, type DriftInput,
} from "./buildDriftReload";

const ORIGIN = "http://127.0.0.1:7686";
const health = (build: string | null, apiCompatible = true): DaemonIdentity => ({ kind: "health", build, apiCompatible });
const base: DriftInput = {
  isWeb: true, releasePage: false, relayedTrigger: false, loadedBuild: "old", origin: ORIGIN,
  daemon: health("new"), visible: true, record: null, now: 42,
};
const decide = (patch: Partial<DriftInput>) => decideBuildDrift({ ...base, ...patch });

class MemoryStorage {
  values = new Map<string, string>();
  getItem(key: string) { return this.values.get(key) ?? null; }
  setItem(key: string, value: string) { this.values.set(key, value); }
  removeItem(key: string) { this.values.delete(key); }
}

describe("build drift decision table", () => {
  it("reloads once for a differing daemon build and records the attempt for that origin and build", () => {
    expect(decide({})).toEqual({ action: "reload", record: { origin: ORIGIN, build: "new", at: 42 } });
  });
  it("shows the banner instead of a second reload for the same daemon build", () => {
    expect(decide({ record: { origin: ORIGIN, build: "new", at: 1 } })).toEqual({ action: "banner", reason: "build" });
  });
  it("reloads again for a newer daemon build or for a record from another daemon origin", () => {
    expect(decide({ record: { origin: ORIGIN, build: "older", at: 1 } }).action).toBe("reload");
    expect(decide({ record: { origin: "http://other:7686", build: "new", at: 1 } }).action).toBe("reload");
  });
  it("clears the record when the loaded bundle matches the daemon", () => {
    expect(decide({ daemon: health("old"), record: { origin: ORIGIN, build: "old", at: 1 } })).toEqual({ action: "clear" });
    expect(decide({ daemon: { kind: "hint", build: "old" } })).toEqual({ action: "clear" });
  });
  it("defers a reload while the page is hidden", () => {
    expect(decide({ visible: false })).toEqual({ action: "defer" });
    expect(decide({ visible: false, record: { origin: ORIGIN, build: "new", at: 1 } })).toEqual({ action: "banner", reason: "build" });
  });
  it("never reloads desktop, signed release pages or relayed peer triggers", () => {
    expect(decide({ isWeb: false })).toEqual({ action: "none" });
    expect(decide({ releasePage: true })).toEqual({ action: "none" });
    expect(decide({ relayedTrigger: true, daemon: { kind: "hint", build: "peer-build" } })).toEqual({ action: "none" });
  });
  it("keeps the API check ahead of every exemption", () => {
    for (const patch of [{}, { isWeb: false }, { releasePage: true }, { daemon: health("old", false) }]) {
      expect(decide({ daemon: health("new", false), ...patch })).toEqual({ action: "banner", reason: "api" });
    }
  });
  it("ignores unknown and dev hashes on either side", () => {
    for (const loadedBuild of ["dev", "unknown", ""]) expect(decide({ loadedBuild })).toEqual({ action: "none" });
    for (const build of ["dev", "unknown", null]) expect(decide({ daemon: health(build) })).toEqual({ action: "none" });
  });
  it("decides from the retained record when the daemon is unreachable after a reload", () => {
    const unreachable: DaemonIdentity = { kind: "unreachable" };
    expect(decide({ daemon: unreachable })).toEqual({ action: "none" });
    expect(decide({ daemon: unreachable, record: { origin: ORIGIN, build: "new", at: 1 } })).toEqual({ action: "banner", reason: "build-unreachable" });
    expect(decide({ daemon: unreachable, loadedBuild: "new", record: { origin: ORIGIN, build: "new", at: 1 } })).toEqual({ action: "clear" });
    expect(decide({ daemon: unreachable, record: { origin: "http://other:7686", build: "new", at: 1 } })).toEqual({ action: "none" });
  });
});

describe("build drift record", () => {
  it("persists only on a byte-equal read-back", () => {
    const storage = new MemoryStorage();
    const record = { origin: ORIGIN, build: "new", at: 1 };
    expect(persistDriftRecord(storage, record)).toBe(true);
    expect(readDriftRecord(storage)).toEqual(record);
    const dropping = { getItem: () => null, setItem: () => {} };
    expect(persistDriftRecord(dropping, record)).toBe(false);
    const throwing = { getItem: () => null, setItem: () => { throw new Error("quota"); } };
    expect(persistDriftRecord(throwing, record)).toBe(false);
  });
  it("treats a malformed record as absent", () => {
    const storage = new MemoryStorage();
    storage.setItem(BUILD_DRIFT_RECORD_KEY, "{");
    expect(readDriftRecord(storage)).toBeNull();
    storage.setItem(BUILD_DRIFT_RECORD_KEY, JSON.stringify({ origin: ORIGIN }));
    expect(readDriftRecord(storage)).toBeNull();
  });
  it("identifies a signed release page by its boot ticket", () => {
    const storage = new MemoryStorage();
    expect(isSignedReleasePage(storage, "boot")).toBe(false);
    storage.setItem("boot", JSON.stringify({ ticket: { releaseId: "frontend-v1.0.0", generation: 1 } }));
    expect(isSignedReleasePage(storage, "boot")).toBe(true);
    storage.setItem("boot", JSON.stringify({ ticket: {} }));
    expect(isSignedReleasePage(storage, "boot")).toBe(false);
  });
});
