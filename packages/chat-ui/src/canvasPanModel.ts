export type CanvasPanDecision = "pan" | "pass";

/** Middle always pans; Space overrides item gestures unless an editor or canvas HTML frame owns focus. */
export function canvasPanDecision({ button, spaceHeld, overItem, typingFocus }: {
  button: number;
  spaceHeld: boolean;
  overItem: boolean;
  typingFocus: boolean;
}): CanvasPanDecision {
  if (button === 1) return "pan";
  if (button !== 0) return "pass";
  if (spaceHeld) return typingFocus ? "pass" : "pan";
  return overItem ? "pass" : "pan";
}
