import type { Plan } from "../types";

/** The plan a detail request names. No id means the selected plan; an unknown id opens nothing. */
export function planForDetail(
  id: string | undefined,
  selected: Pick<Plan, "id" | "team_id"> | null,
  ...sources: ReadonlyArray<readonly Pick<Plan, "id" | "team_id">[]>
): Pick<Plan, "id" | "team_id"> | null {
  if (id === undefined) return selected;
  for (const plans of sources) {
    const found = plans.find((plan) => plan.id === id);
    if (found) return found;
  }
  return null;
}
