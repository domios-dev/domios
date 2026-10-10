import type { BlueprintSnapshot } from "./types";

export function runningBlueprintNamesForOwner(
  blueprints: readonly BlueprintSnapshot[],
  kind: "tab" | "team",
  ownerId: string,
): string[] {
  return blueprints
    .filter((blueprint) => blueprint.state === "running" && blueprint.owner?.kind === kind
      && (kind === "tab" ? blueprint.owner.tab_id : blueprint.owner.team_id) === ownerId)
    .map((blueprint) => blueprint.name);
}
