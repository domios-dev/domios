import { expect, it } from "vitest";
import { runtimeActivityLabel } from "./agentRuntimeTiming";
import { runtimeStateFromSnapshot } from "./agentRuntimeState";

it("keeps active and completed timing through snapshot hydration and projects elapsed on render", () => {
  const state = runtimeStateFromSnapshot(null, {
    state: "loaded", epoch: 2, active_turn_started_at_ms: 100_000, last_turn_duration_ms: 12_000,
  });
  expect(state).toEqual({ state: "loaded", epoch: 2, active_turn_started_at_ms: 100_000, last_turn_duration_ms: 12_000 });
  expect(runtimeActivityLabel("fixture", "working", state, 165_000)).toBe("fixture · Elapsed 1m 5s");
  expect(runtimeActivityLabel("fixture", "working", state, 166_000)).toBe("fixture · Elapsed 1m 6s");
  expect(runtimeActivityLabel("fixture", "idle", state, 166_000)).toBe("fixture · Last turn 12s");
});

it("keeps unknown timing and non-runtime status unchanged", () => {
  expect(runtimeActivityLabel("fixture", "working", null, 100)).toBe("fixture");
  expect(runtimeActivityLabel("fixture", "working", { state: "loaded", epoch: 2 }, 100)).toBe("fixture");
  expect(runtimeActivityLabel("fixture", "working", { state: "degraded", epoch: 2, active_turn_started_at_ms: 0 }, 100)).toBe("fixture");
  expect(runtimeActivityLabel("fixture", "waiting-on-user", { state: "loaded", epoch: 2, last_turn_duration_ms: 10 }, 100)).toBe("fixture");
});

it.each([NaN, Infinity, -1])("rejects invalid start or duration %s", (value) => {
  const runtime = { state: "loaded" as const, epoch: 1, active_turn_started_at_ms: value, last_turn_duration_ms: value };
  expect(runtimeActivityLabel("fixture", "working", runtime, 100)).toBe("fixture");
  expect(runtimeActivityLabel("fixture", "idle", runtime, 100)).toBe("fixture");
});

it("clamps provider clock skew to zero elapsed", () => {
  expect(runtimeActivityLabel("fixture", "working", { state: "loaded", epoch: 1, active_turn_started_at_ms: 1000 }, 900)).toBe("fixture · Elapsed 0s");
});
