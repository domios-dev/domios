import assert from "node:assert/strict";
import { test } from "node:test";
import {
  RECONNECT_GRACE_MS,
  RECONNECTED_NOTICE_MS,
  brandConnectionModel,
  brandStateModel,
  transitionBrandConnection,
} from "./brandStateModel";
import {
  DOT_MATRIX_GRID_STEP,
  DOT_MATRIX_REST_ALPHA,
  DOT_MATRIX_REST_RADIUS,
  DOT_MATRIX_WAVE_BAND_WIDTH,
  DOT_MATRIX_WAVE_DURATION_MS,
  DOT_MATRIX_WAVE_GROWTH,
  DOT_MATRIX_WAVE_PEAK_ALPHA,
  emitDotMatrixWave,
  sampleDotMatrixWave,
} from "./dotMatrixModel";
import type { DotMatrixWave } from "./dotMatrixModel";

test("first connect remains hidden until the exact grace deadline, then blocks", () => {
  const disconnected = transitionBrandConnection({ kind: "connected" }, "disconnect", 500);
  assert.deepEqual(brandConnectionModel(disconnected, 500 + RECONNECT_GRACE_MS - 1, false), { kind: "hidden" });
  assert.deepEqual(brandConnectionModel(disconnected, 500 + RECONNECT_GRACE_MS, false), { kind: "reconnecting" });
  assert.equal(transitionBrandConnection(disconnected, "grace-expired", 500 + RECONNECT_GRACE_MS).kind, "disconnected");
});

test("a short disconnect reconnects quietly and cancels the pending deadline", () => {
  const waiting = transitionBrandConnection({ kind: "connected" }, "disconnect", 100);
  const connected = transitionBrandConnection(waiting, "connect", 9_999);
  const lateDeadline = transitionBrandConnection(connected, "grace-expired", 10_100);
  assert.equal(lateDeadline.kind, "connected");
  assert.deepEqual(brandConnectionModel(connected, 9_999), { kind: "hidden" });
});

test("a disconnect that outlasts the grace period shows a reconnect notice", () => {
  const waiting = transitionBrandConnection({ kind: "connected" }, "disconnect", 100);
  const reconnected = transitionBrandConnection(waiting, "connect", 100 + RECONNECT_GRACE_MS);
  assert.deepEqual(reconnected, { kind: "reconnected", connectedAt: 100 + RECONNECT_GRACE_MS });
  assert.deepEqual(brandConnectionModel(reconnected, 100 + RECONNECT_GRACE_MS), { kind: "reconnected" });
  assert.deepEqual(brandConnectionModel(reconnected, 100 + RECONNECT_GRACE_MS + RECONNECTED_NOTICE_MS), { kind: "hidden" });
});

test("a drop after the app loaded pauses updates without blocking; only a first connect blocks", () => {
  const waiting = transitionBrandConnection({ kind: "connected" }, "disconnect", 100);
  const lost = transitionBrandConnection(waiting, "grace-expired", 100 + RECONNECT_GRACE_MS);
  assert.deepEqual(brandConnectionModel(waiting, 100 + RECONNECT_GRACE_MS - 1, true), { kind: "hidden" });
  assert.deepEqual(brandConnectionModel(waiting, 100 + RECONNECT_GRACE_MS, true), { kind: "paused" });
  assert.deepEqual(brandConnectionModel(lost, 100 + RECONNECT_GRACE_MS, true), { kind: "paused" });
  assert.deepEqual(brandConnectionModel(lost, 100 + RECONNECT_GRACE_MS, false), { kind: "reconnecting" });
  const back = 200 + RECONNECT_GRACE_MS;
  const recovered = transitionBrandConnection(lost, "connect", back);
  assert.deepEqual(brandConnectionModel(recovered, back, true), { kind: "reconnected" }, "recovery replaces the pill with the notice");
  assert.deepEqual(brandConnectionModel(recovered, back + RECONNECTED_NOTICE_MS, true), { kind: "hidden" });
});

test("error presentation takes precedence over loading", () => {
  assert.deepEqual(brandStateModel({
    loading: true,
    label: "Loading",
    error: { title: "Could not start", sentence: "Retry the session." },
  }), { kind: "error", title: "Could not start", sentence: "Retry the session." });
  assert.deepEqual(brandStateModel({ loading: true, label: "Restoring workspace" }), {
    kind: "loading",
    label: "Restoring workspace",
    animate: true,
  });
});

test("each loading release emits one timed matrix wave", () => {
  const first: DotMatrixWave = { releasedAt: 100, originX: 50, originY: 40, maxRadius: 100 };
  const second: DotMatrixWave = { ...first, releasedAt: 3_700 };
  const waves = emitDotMatrixWave([], first, "loading", false);
  const afterSecondRelease = emitDotMatrixWave(waves, second, "loading", false);
  assert.equal(afterSecondRelease.length, 2);
  assert.deepEqual(sampleDotMatrixWave(first, 100 + DOT_MATRIX_WAVE_DURATION_MS / 2), {
    progress: 0.5,
    radius: 50,
    fade: 0.5,
  });
  assert.equal(sampleDotMatrixWave(first, 100 + DOT_MATRIX_WAVE_DURATION_MS), null);
  assert.equal(DOT_MATRIX_WAVE_PEAK_ALPHA, 0.16);
  assert.equal(DOT_MATRIX_WAVE_GROWTH, 0.3);
  assert.equal(DOT_MATRIX_WAVE_BAND_WIDTH, 26);
  assert.equal(DOT_MATRIX_GRID_STEP, 18);
  assert.equal(DOT_MATRIX_REST_ALPHA, 0.018);
  assert.equal(DOT_MATRIX_REST_RADIUS, 1.2);
});

test("error and reduced-motion states emit no matrix waves", () => {
  const wave: DotMatrixWave = { releasedAt: 1_000, originX: 12, originY: 12, maxRadius: 80 };
  const waves = [wave];
  assert.equal(emitDotMatrixWave(waves, { ...wave, releasedAt: 2_000 }, "error", false), waves);
  assert.equal(emitDotMatrixWave(waves, { ...wave, releasedAt: 2_000 }, "loading", true), waves);
});
