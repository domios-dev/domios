import assert from "node:assert/strict";
import test from "node:test";
import { nextCameraRest, sharpFrameZoom, SHARP_FRAME_MIN_SCALE, type CameraRest, type CameraRestEvent } from "./canvasRest";

const walk = (events: CameraRestEvent[], start: CameraRest = null) => {
  const zooms: number[] = [];
  let rest = start;
  for (const event of events) { rest = nextCameraRest(rest, event); zooms.push(sharpFrameZoom(rest)); }
  return zooms;
};

test("frames sharpen when the camera settles, go plain when a zoom starts, and stay sharp while panning", () => {
  assert.deepEqual(walk([
    { kind: "settle", scale: 0.93 },
    { kind: "paint", scale: 0.93 },
    { kind: "paint", scale: 0.9 },
    { kind: "paint", scale: 0.93 },
    { kind: "settle", scale: 0.8 },
    { kind: "paint", scale: 0.8 },
    { kind: "settle", scale: 0.8 },
  ]), [0.93, 0.93, 1, 1, 0.8, 0.8, 0.8]);
});

test("frames stay plain before the first settle and at scales where sharpening does not apply", () => {
  assert.deepEqual(walk([{ kind: "paint", scale: 0.7 }]), [1]);
  assert.deepEqual([1, 2, SHARP_FRAME_MIN_SCALE - 0.01, SHARP_FRAME_MIN_SCALE].map((scale) => sharpFrameZoom(scale)), [1, 1, 1, SHARP_FRAME_MIN_SCALE]);
});
