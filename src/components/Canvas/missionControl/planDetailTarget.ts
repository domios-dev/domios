import type { Plan } from "../types";

/** The plan a detail request names. No id means the selected plan; an unknown id opens nothing. */
export function planForDetail(
  id: string | undefined,
  selected: Plan | null,
  ...sources: ReadonlyArray<readonly Plan[]>
): Plan | null {
  if (id === undefined) return selected;
  for (const plans of sources) {
    const found = plans.find((plan) => plan.id === id);
    if (found) return found;
  }
  return null;
}
