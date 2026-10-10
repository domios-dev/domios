import type { AutomationGraph, BlueprintSnapshot } from "../../../../src/components/Automation/types";
import { JobsView, type JobsViewProps } from "../../../../src/components/Jobs/JobsView";
import type { CronJob, CronRunStatus } from "../../../../src/lib/types";
import { provideForWorkspace } from "../../mocks/backend";
import { PinnedMenu } from "../stageTools";
import { defineSpecimen } from "../types";

const MINUTE = 60_000;
const JOBS_WORKSPACE = "gallery-jobs";
const EMPTY_WORKSPACE = "gallery-jobs-empty";
const WORKSPACES = [{ id: JOBS_WORKSPACE, name: "CodeTerm" }, { id: EMPTY_WORKSPACE, name: "Docs site" }];

function run(status: CronRunStatus, minutesAgo: number): CronJob["last_run"] {
  const started = Date.now() - minutesAgo * MINUTE;
  return { run_id: `run-${status}-${minutesAgo}`, started_at: started, finished_at: status === "Running" ? null : started + 4 * MINUTE, status, tab_id: null };
}

function cron(id: string, name: string, schedule: string, enabled: boolean, lastRun: CronJob["last_run"]): CronJob {
  return {
    id, name, description: null, workspace_id: JOBS_WORKSPACE, team_id: null, schedule, enabled, agent: "claude", model: null,
    reasoning_effort: null, task: name, cwd: null, role: null, report_target_tab_id: null, oneshot: false, max_history: 10,
    created_at: 1, next_run: enabled ? Date.now() + 40 * MINUTE : null, jitter_max_ms: null, last_run: lastRun,
  };
}

function blueprint(id: string, name: string, state: BlueprintSnapshot["state"], trigger: NonNullable<BlueprintSnapshot["trigger"]>): BlueprintSnapshot {
  return {
    id, name, owner: { kind: "tab", tab_id: "jobs-owner", team_id: null }, state, run_id: state === "running" ? `run-${id}` : null,
    current_step: state === "running" ? 0 : null, last_event_at_ms: Date.now() - 6 * MINUTE, trigger,
    steps: [{ index: 0, name: "Collect", kind: "command", title: "Collect", state: state === "running" ? "running" : state === "failed" ? "failed" : "waiting", outcome: null, started_at_ms: null, ended_at_ms: null, result_summary: null, reason: state === "failed" ? "exit 1" : null }],
    result: { target: { kind: "owner_tab", id: "jobs-owner" }, state: state === "failed" ? "failed" : "waiting", reason: null, body_ref: null },
  };
}

function graph(workspaceId: string, blueprints: BlueprintSnapshot[]): AutomationGraph {
  return { workspace_id: workspaceId, team_id: null, all_teams: true, revision: "gallery", scenarios: [], nodes: [], edges: [], blueprints };
}

provideForWorkspace("cron_list", JOBS_WORKSPACE, [
  cron("jobs-digest", "Morning digest", "0 9 * * *", true, run("Success", 22 * 60)),
  cron("jobs-triage", "Inbox triage", "*/15 * * * *", true, run("Running", 2)),
  cron("jobs-backup", "Nightly backup", "30 2 * * *", true, run("Failed", 8 * 60)),
  cron("jobs-release", "Release notes", "0 17 * * 5", false, run("Success", 3 * 24 * 60)),
]);
provideForWorkspace("automation_graph_workspace_snapshot", JOBS_WORKSPACE, graph(JOBS_WORKSPACE, [
  blueprint("jobs-train", "Nightly release train", "running", { type: "schedule", summary: "Every night at 01:00" }),
  blueprint("jobs-qa", "QA regression sweep", "failed", { type: "event", summary: "When a PR is merged" }),
  blueprint("jobs-migration", "Legacy migration", "stopped", { type: "input", summary: "Started by hand" }),
]));
provideForWorkspace("cron_list", EMPTY_WORKSPACE, []);
provideForWorkspace("automation_graph_workspace_snapshot", EMPTY_WORKSPACE, graph(EMPTY_WORKSPACE, []));

type JobsSpecimenProps = Omit<JobsViewProps, "workspaces" | "teams"> & { menu?: boolean };

function JobsSpecimen({ menu = false, ...props }: JobsSpecimenProps) {
  const view = <div className="gallery-fill gallery-panel"><JobsView workspaces={WORKSPACES} teams={[]} {...props} /></div>;
  return menu ? <PinnedMenu trigger='[aria-label="Actions for Nightly release train"]'>{view}</PinnedMenu> : view;
}

const close = () => undefined;
const filter = (workspaceId: string) => ({ workspaceId, teamId: null });

export default defineSpecimen({
  id: "jobs-list",
  title: "Jobs list",
  category: "Automation",
  where: "src/components/Jobs/JobsView.tsx",
  stage: "full",
  notes: "The shipped JobsView over mocked cron_list and automation_graph_workspace_snapshot, scoped to a gallery workspace: crons scheduled, running, with a failed last run and paused, beside Blueprints active, failed and stopped, on the shared ct-card rows with kind tones.",
  Component: JobsSpecimen,
  states: [
    { id: "mixed", label: "Mixed crons + Blueprints · Back pill", props: { initialFilter: filter(JOBS_WORKSPACE), onClose: close } },
    { id: "no-back", label: "Opened without a return path · no Back", props: { initialFilter: filter(JOBS_WORKSPACE) } },
    { id: "row-menu", label: "Row actions open", props: { initialFilter: filter(JOBS_WORKSPACE), onClose: close, menu: true } },
    { id: "empty", label: "No jobs in the workspace", props: { initialFilter: filter(EMPTY_WORKSPACE), onClose: close } },
  ],
});
