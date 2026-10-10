import { FIXTURE_PLAN, FIXTURE_TASK } from "../../../../src/components/Canvas/cards/fixtures.testdata";
import type { PlanIndexEntry } from "../../../../src/components/Canvas/plansApi";
import type { Plan } from "../../../../src/components/Canvas/types";
import type { AutomationGraph, BlueprintSnapshot } from "../../../../src/components/Automation/types";
import type { DelayedMessage } from "../../../../src/components/Terminal/delayedSend";
import type { CronJob } from "../../../../src/lib/types";
import { mockCommand, provideForTab, provideList } from "../../mocks/backend";
import { ChatBottomBar, type ChatBottomBarProps } from "../ChatBottomBar";
import { agentTab, WORKSPACE_ID } from "../fixtures";
import { defineSpecimen } from "../types";

const MINUTE = 60_000;
const ids = { plan: "ctx-plan", task: "ctx-task", kinds: "ctx-kinds", waiting: "ctx-waiting", stashed: "ctx-stashed" };

function ownedPlan(id: string, title: string, owner: string, tasks: Plan["tasks"] = []): Plan {
  return { ...FIXTURE_PLAN, id, title, owner: { tab_id: owner, session_id: null, title: null }, project_path: "/Developer/codeterm", team_id: null, tasks };
}

function indexEntry(plan: Plan, assigned: string[] = []): PlanIndexEntry {
  return { plan_id: plan.id, file_path: `/plans/${plan.id}.json`, owner: plan.owner, title: plan.title, status: "active", updated_at: 1, team_id: null, assigned_tab_ids: assigned, task_count: plan.tasks.length };
}

const plans = [
  ownedPlan("ctx-gallery", "Gallery refresh", ids.plan),
  ownedPlan("ctx-gallery-kinds", "Gallery refresh", ids.kinds),
  ownedPlan("ctx-docs-kinds", "Docs sync", ids.kinds),
  ownedPlan("ctx-gallery-task", "Gallery refresh", "ctx-lead", [{ ...FIXTURE_TASK, id: "ctx-task-1", name: "Mirror composer context header", status: "in_progress", assigned_tab_id: ids.task }]),
];
provideList("list_plans_full", plans);
provideList("list_plan_index", [indexEntry(plans[0]), indexEntry(plans[1]), indexEntry(plans[2]), indexEntry(plans[3], [ids.task])]);

function cron(id: string, name: string, tab: string, run: CronJob["last_run"] = null): CronJob {
  return {
    id, name, description: null, workspace_id: WORKSPACE_ID, team_id: null, schedule: "0 9 * * *", enabled: true, agent: "claude", model: null,
    reasoning_effort: null, task: name, cwd: null, role: null, creator_tab_id: tab, report_target_tab_id: null, oneshot: false, max_history: 10,
    created_at: 1, next_run: Date.now() + 40 * MINUTE, jitter_max_ms: null, last_run: run,
  };
}

const awaiting = (tab: string) => ({ run_id: `run-${tab}`, started_at: 1, finished_at: null, status: "AwaitingOwnerPrompt", tab_id: tab }) as CronJob["last_run"];
provideList("cron_list", [
  cron("ctx-digest", "Morning digest", ids.kinds),
  cron("ctx-triage", "Inbox triage", ids.kinds, awaiting(ids.kinds)),
  cron("ctx-triage-wait", "Inbox triage", ids.waiting, awaiting(ids.waiting)),
  cron("ctx-release-wait", "Release notes", ids.waiting),
]);

function blueprint(id: string, name: string, owner: string, state: BlueprintSnapshot["state"]): BlueprintSnapshot {
  return {
    id, name, owner: { kind: "tab", tab_id: owner, team_id: null }, state, run_id: `run-${id}`, current_step: 0, last_event_at_ms: 100,
    trigger: { type: "event", summary: "Agent message" },
    steps: [{ index: 0, name: "Echo", kind: "command", title: "Echo", state: state === "running" ? "running" : "waiting", outcome: null, started_at_ms: 50, ended_at_ms: null, result_summary: null, reason: null }],
    result: { target: { kind: "owner_tab", id: owner }, state: "waiting", reason: null, body_ref: null },
  } as BlueprintSnapshot;
}

mockCommand("automation_graph_workspace_snapshot", (args) => ({
  workspace_id: String(args?.workspaceId ?? WORKSPACE_ID), team_id: null, all_teams: true, revision: "gallery", scenarios: [], nodes: [], edges: [],
  blueprints: [blueprint("ctx-release", "Nightly release train", ids.kinds, "running"), blueprint("ctx-migration", "Legacy migration", ids.kinds, "stopped")],
}) as AutomationGraph);

function delayed(tabId: string, id: string, text: string, minutes: number | null): DelayedMessage {
  return { id, tabId, text, scheduledAtMs: Date.now() + (minutes ?? 15) * MINUTE, createdAtMs: 1, state: minutes == null ? "stashed" : "queued", sessionChanged: false } as DelayedMessage;
}

provideForTab("delayed_send_list", ids.kinds, [
  delayed(ids.kinds, "ctx-d1", "Check whether the release build finished and post the summary", 12),
  delayed(ids.kinds, "ctx-d2", "Standup notes", 120),
]);
provideForTab("delayed_send_list", ids.stashed, [
  delayed(ids.stashed, "ctx-s1", "Check whether the release build finished and post the summary", 12),
  delayed(ids.stashed, "ctx-s2", "Draft the changelog once QA signs off", null),
]);

const tab = (id: string) => agentTab(id, { title: "Composer" });

export default defineSpecimen({
  id: "composer-context-header",
  title: "Composer context header",
  stage: "wide",
  category: "Composer",
  where: "src/components/Terminal/ComposerContextHeader.tsx",
  notes: "The real ChatComposer portals the real ComposerContextHeader into TabSessionChat's context slot. Plans, crons, blueprints and delayed messages come from mocked IPC fixtures; open strips are opened through the kind buttons.",
  Component: (props: ChatBottomBarProps) => <ChatBottomBar showStatus={false} {...props} />,
  states: [
    { id: "plan", label: "Plan only", props: { tab: tab(ids.plan) } },
    { id: "task", label: "Task title", props: { tab: tab(ids.task) } },
    { id: "kinds", label: "Plan +1 · three kinds closed", props: { tab: tab(ids.kinds) } },
    { id: "blueprints", label: "Blueprints open", props: { tab: tab(ids.kinds), openKind: "blueprints" } },
    { id: "crons", label: "Crons open · waiting on you first", props: { tab: tab(ids.kinds), openKind: "crons" } },
    { id: "delayed", label: "Delayed messages open", props: { tab: tab(ids.kinds), openKind: "delayed" } },
    { id: "cron-waiting", label: "Cron waiting on you · no plan", props: { tab: tab(ids.waiting), openKind: "crons" } },
    { id: "stashed", label: "Stashed delayed message", props: { tab: tab(ids.stashed), openKind: "delayed" } },
    { id: "typing", label: "Typing folds the strip (loops)", props: { tab: tab(ids.kinds), typingLoop: true } },
    { id: "narrow", label: "Narrow width", props: { tab: tab(ids.kinds), openKind: "delayed", narrow: true } },
  ],
});
