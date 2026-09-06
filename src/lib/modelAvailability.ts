import type { ModelInfo } from "./types";

/**
 * The one availability answer every surface renders, so `codeterm agent models`
 * and the picker cannot disagree. `unknown` is what a probe that could not run
 * reports: unavailable means the CLI said no, never that we failed to ask.
 */
export type ModelAvailability = "available" | "unavailable" | "unknown";

export function modelAvailability(model: Pick<ModelInfo, "available" | "availability">): ModelAvailability {
  if (model.availability) return model.availability;
  return model.available === false ? "unavailable" : "available";
}

/** A model whose availability could not be established is offered, not blocked. */
export function isModelSelectable(model: Pick<ModelInfo, "available" | "availability">): boolean {
  return modelAvailability(model) !== "unavailable";
}
