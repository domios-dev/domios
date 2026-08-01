import type { Tab, Team } from "../../lib/types";
import type { PlanIndexEntry } from "../Canvas/plansApi";
import { FIXTURE_PLAN, FIXTURE_TASK } from "../Canvas/cards/fixtures.testdata";
import type { Plan, Task } from "../Canvas/types";

export function tab(overrides: Partial<Tab> & { team_id?: string | null }): Tab;
export function tab(_rowId: string, tabId: string, _sortOrder: number): string;
export function tab(
  overridesOrRowId: Partial<Tab> & { team_id?: string | null } | string,
  tabId?: string,
  _sortOrder?: number,
): Tab | string {
  if (typeof overridesOrRowId === "string") return tabId!;
  return {
    id: "tab",
    pty_id: null,
    tab_type: "Terminal",
    title: "Tab",
    cwd: null,
    role: null,
    role_profile: null,
    parent_tab_id: null,
    ...overridesOrRowId,
  } as Tab;
}

export function task(id: string, status: Task["status"]): Task {
  return { ...FIXTURE_TASK, id, name: id, status };
}

export function plan(tasks: Task[]): Plan {
  return {
    ...FIXTURE_PLAN,
    id: "plan-team",
    title: "Sidebar team effort",
    team_id: "team-one",
    tasks,
  } as Plan;
}

export const TEAM: Team = {
  id: "team-one",
  name: "Sidebar team",
  lifecycle: "active",
  created_at: 1,
  workspace_id: "workspace",
};

export function planIndexEntry(
  planId = "plan-team",
  teamId: string | null = TEAM.id,
): PlanIndexEntry {
  return {
    plan_id: planId,
    file_path: "/workspace/.codeterm/plans/active/plan-team/plan.json",
    canonical_project_path: "/workspace",
    owner: { tab_id: "orch", session_id: null, title: null },
    title: "Sidebar team effort",
    status: "active",
    updated_at: 1,
    team_id: teamId,
    assigned_tab_ids: ["orch"],
    task_count: 1,
  };
}
