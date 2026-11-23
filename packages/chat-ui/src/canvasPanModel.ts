export type CanvasPanDecision = "pan" | "pass";
export type CanvasPanMode = "active" | "ready" | undefined;

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

export function canvasPanMode({ panning, spaceHeld, typingFocus }: {
  panning: boolean;
  spaceHeld: boolean;
  typingFocus: boolean;
}): CanvasPanMode {
  if (panning) return "active";
  if (spaceHeld && !typingFocus) return "ready";
  return undefined;
}

export function canvasPanCursor(mode: CanvasPanMode): "grabbing" | "grab" | "default" {
  if (mode === "active") return "grabbing";
  if (mode === "ready") return "grab";
  return "default";
}
