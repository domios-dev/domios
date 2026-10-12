import { KeyboardSensor, MouseSensor, TouchSensor } from "@dnd-kit/core";
import { sortableKeyboardCoordinates } from "@dnd-kit/sortable";

export const POINTER_DRAG_ACTIVATION_DISTANCE = 4;

export function sortableSensorConfiguration(pointerType: "mouse" | "touch") {
  return {
    sensor: pointerType === "touch" ? TouchSensor : MouseSensor,
    options: {
      activationConstraint: pointerType === "touch"
        ? { delay: 400, tolerance: 8 }
        : { distance: POINTER_DRAG_ACTIVATION_DISTANCE },
    },
    keyboard: {
      sensor: KeyboardSensor,
      options: { coordinateGetter: sortableKeyboardCoordinates },
    },
  };
}
