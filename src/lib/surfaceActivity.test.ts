import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createSurfaceActivityStore, parseSurfaceActivity, SURFACE_ACTOR_LINGER_MS, SURFACE_PULSE_MS } from "./surfaceActivity";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("surface activity pulses", () => {
  it("runs one pass per verb and lands after the pass duration", () => {
    const store = createSurfaceActivityStore();
    const changes = vi.fn();
    store.subscribe(changes);
    store.record({ tab_id: "s1", verb: "click" });
    expect(store.pulse("s1")).toEqual({ nonce: 1, verb: "click" });
    vi.advanceTimersByTime(SURFACE_PULSE_MS - 1);
    expect(store.pulse("s1")).not.toBeNull();
    vi.advanceTimersByTime(1);
    expect(store.pulse("s1")).toBeNull();
    expect(changes).toHaveBeenCalledTimes(2);
    vi.advanceTimersByTime(SURFACE_PULSE_MS * 4);
    expect(store.pulse("s1")).toBeNull();
    expect(changes).toHaveBeenCalledTimes(2);
  });

  it("restarts the pass on a second verb instead of stacking passes", () => {
    const store = createSurfaceActivityStore();
    store.record({ tab_id: "s1", verb: "goto" });
    vi.advanceTimersByTime(SURFACE_PULSE_MS / 2);
    store.record({ tab_id: "s1", verb: "type" });
    expect(store.pulse("s1")).toEqual({ nonce: 2, verb: "type" });
    vi.advanceTimersByTime(SURFACE_PULSE_MS / 2);
    expect(store.pulse("s1")?.nonce).toBe(2);
    vi.advanceTimersByTime(SURFACE_PULSE_MS / 2);
    expect(store.pulse("s1")).toBeNull();
  });

  it("keeps surfaces independent", () => {
    const store = createSurfaceActivityStore();
    store.record({ tab_id: "s1", verb: "click" });
    expect(store.pulse("s2")).toBeNull();
    store.record({ tab_id: "s2", verb: "click" });
    vi.advanceTimersByTime(SURFACE_PULSE_MS);
    expect(store.pulse("s1")).toBeNull();
    expect(store.pulse("s2")).toBeNull();
  });

  it("ignores malformed events", () => {
    const store = createSurfaceActivityStore();
    const changes = vi.fn();
    store.subscribe(changes);
    for (const payload of [null, "s1", {}, { tab_id: "" }, { tab_id: 3, verb: "x" }]) store.record(payload);
    expect(changes).not.toHaveBeenCalled();
    expect(parseSurfaceActivity({ tab_id: "s1" })).toEqual({ tab_id: "s1", verb: "" });
  });

  it("names a visiting agent while it acts and drops it when the owner acts or it goes quiet", () => {
    const store = createSurfaceActivityStore();
    store.record({ tab_id: "s1", verb: "click", actor: " QA " });
    expect(store.pulse("s1")).toEqual({ nonce: 1, verb: "click" });
    expect(store.actor("s1")).toBe("QA");
    vi.advanceTimersByTime(SURFACE_ACTOR_LINGER_MS - 1);
    expect(store.actor("s1")).toBe("QA");
    vi.advanceTimersByTime(1);
    expect(store.actor("s1")).toBeNull();
    store.record({ tab_id: "s1", verb: "type", actor: "QA" });
    store.record({ tab_id: "s1", verb: "goto" });
    expect(store.actor("s1")).toBeNull();
    expect(parseSurfaceActivity({ tab_id: "s1", verb: "x", actor: "" })).toEqual({ tab_id: "s1", verb: "x" });
    expect(parseSurfaceActivity({ tab_id: "s1", verb: "x", actor: "y".repeat(80) })?.actor).toHaveLength(48);
  });
});
