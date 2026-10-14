import assert from "node:assert/strict";
import { test } from "node:test";
import { KeyboardSensor, MouseSensor, TouchSensor } from "@dnd-kit/core";
import { sortableKeyboardCoordinates } from "@dnd-kit/sortable";
import { sortableSensorConfiguration } from "./sortableSensors";

test("touch drag requires a held press within movement tolerance", () => {
  const configuration = sortableSensorConfiguration("touch");
  assert.equal(configuration.sensor, TouchSensor);
  assert.deepEqual(configuration.options.activationConstraint, { delay: 400, tolerance: 8 });
});

test("mouse and trackpad drag keep the four-pixel activation distance", () => {
  const configuration = sortableSensorConfiguration("mouse");
  assert.equal(configuration.sensor, MouseSensor);
  assert.deepEqual(configuration.options.activationConstraint, { distance: 4 });
});

test("keyboard reordering remains available for either pointer type", () => {
  for (const pointerType of ["mouse", "touch"] as const) {
    const configuration = sortableSensorConfiguration(pointerType);
    assert.equal(configuration.keyboard.sensor, KeyboardSensor);
    assert.equal(configuration.keyboard.options.coordinateGetter, sortableKeyboardCoordinates);
  }
});
