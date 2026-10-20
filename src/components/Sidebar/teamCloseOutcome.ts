import type { TeamCloseInventory } from "../../lib/teamActions";
import type { Tab, CronJob } from "../../lib/types";
import type { BlueprintSnapshot } from "../Automation/types";

export function teamCloseSurvivors(teamId: string, tabs: readonly Tab[], crons: readonly CronJob[], blueprints: readonly BlueprintSnapshot[]): string[] {
  const members = tabs.filter(tab => tab.team_id === teamId);
  const ids = new Set(members.map(tab => tab.id));
  return [
    ...members.map(tab => `Tab: ${tab.title || tab.id}`),
    ...crons.filter(cron => cron.team_id === teamId).map(cron => `Cron: ${cron.name}`),
    ...blueprints.filter(blueprint => blueprint.state === "running" && (
      blueprint.owner?.kind === "team" && blueprint.owner.team_id === teamId
      || blueprint.owner?.kind === "tab" && ids.has(blueprint.owner.tab_id ?? "")
    )).map(blueprint => `Blueprint: ${blueprint.name}`),
  ];
}

export function teamCloseOutcome(result: TeamCloseInventory, survivors: readonly string[] = []) {
  const preserved = [...new Set(result.kept.map(lane => lane.path ?? lane.branch).filter((path): path is string => Boolean(path)))];
  return {
    kind: survivors.length ? "partial" : preserved.length ? "closed_with_preserved_lanes" : "closed",
    survivors: [...survivors],
    preserved,
  } as const;
}

export function startBackgroundClose<T>(dismiss: () => void, close: () => Promise<T>): Promise<T> {
  dismiss();
  return close();
}
