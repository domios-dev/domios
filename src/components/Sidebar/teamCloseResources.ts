import type { CronJob, Tab, Team, Workspace } from "../../lib/types";
import type { Plan, Task } from "../Canvas/types";
import { flattenSidebarTabs } from "./workspaceTabs";

export interface TeamCloseResource {
  id: string;
  name: string;
  members: Array<{ tabId: string; title: string; role: string | null; roleProfile: string | null }>;
  crons: string[];
  lanes: string[];
}

function taskClosesWithTeam(task: Task, planTeamId: string | null | undefined, memberIds: Set<string>, teamId: string): boolean {
  if (task.status === "done" || task.status === "cancelled" || task.status === "in_review") return false;
  const ownsLane = task.assigned_tab_id != null && memberIds.has(task.assigned_tab_id)
    || task.lane?.tab_id != null && memberIds.has(task.lane.tab_id);
  const ownsAttempt = task.execution_attempts?.some((attempt) =>
    attempt.closed_at == null
    && memberIds.has(attempt.executor_tab_id ?? attempt.tab_id ?? ""),
  ) ?? false;
  const orphanedTeamWork = planTeamId === teamId
    && task.status === "in_progress"
    && task.assigned_tab_id == null
    && task.lane?.tab_id == null;
  return ownsLane || ownsAttempt || orphanedTeamWork;
}

export function teamCloseResourcesForWorkspace(
  workspace: Workspace,
  tabs: Record<string, Tab>,
  teams: readonly Team[],
  crons: readonly CronJob[],
  plans: readonly Plan[],
): TeamCloseResource[] {
  const workspaceTabs = flattenSidebarTabs(workspace.tabs, tabs);
  const membersByTeam = new Map<string, Tab[]>();
  for (const team of teams) {
    if (team.workspace_id === workspace.id && !team.remote_peer_name) {
      membersByTeam.set(team.id, []);
    }
  }
  for (const tab of workspaceTabs) {
    if (!tab.team_id) continue;
    const members = membersByTeam.get(tab.team_id) ?? [];
    members.push(tab);
    membersByTeam.set(tab.team_id, members);
  }

  return [...membersByTeam.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([teamId, members]) => {
      const memberIds = new Set(members.map((member) => member.id));
      const team = teams.find((candidate) => candidate.id === teamId);
      const lanes = new Set<string>();
      for (const plan of plans) {
        for (const task of plan.tasks) {
          if (taskClosesWithTeam(task, plan.team_id, memberIds, teamId)) {
            lanes.add(`${plan.title}: ${task.name}`);
          }
        }
      }
      return {
        id: teamId,
        name: team?.name?.trim() || "Team",
        members: members.map((member) => ({
          tabId: member.id,
          title: member.title || "Tab",
          role: member.role ?? null,
          roleProfile: member.role_profile ?? null,
        })),
        crons: crons.filter((cron) => cron.team_id === teamId).map((cron) => cron.name),
        lanes: [...lanes].sort((left, right) => left.localeCompare(right)),
      };
    });
}

export function teamCloseResourcesForId(
  resources: readonly TeamCloseResource[],
  teamId: string,
): TeamCloseResource | null {
  return resources.find((resource) => resource.id === teamId) ?? null;
}
