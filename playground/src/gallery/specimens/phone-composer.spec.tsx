import { FIXTURE_PLAN } from "../../../../src/components/Canvas/cards/fixtures.testdata";
import type { PlanIndexEntry } from "../../../../src/components/Canvas/plansApi";
import type { Plan } from "../../../../src/components/Canvas/types";
import type { AutomationGraph, BlueprintSnapshot } from "../../../../src/components/Automation/types";
import type { DelayedMessage } from "../../../../src/components/Terminal/delayedSend";
import type { CronJob } from "../../../../src/lib/types";
import { provideForTab, provideForWorkspace, provideList } from "../../mocks/backend";
import { ChatBottomBar, type ChatBottomBarProps } from "../ChatBottomBar";
import { agentTab } from "../fixtures";
import { forcePhoneViewport, PhoneViewport } from "../stageTools";
import { defineSpecimen } from "../types";

const SPECIMEN_ID = "phone-composer";
const MINUTE = 60_000;
const PHONE_WORKSPACE = "gallery-phone";
const ids = { planned: "phone-planned", crons: "phone-crons" };

forcePhoneViewport(SPECIMEN_ID);

const plan: Plan = { ...FIXTURE_PLAN, id: "phone-plan", title: "Phone composer polish", owner: { tab_id: ids.planned, session_id: null, title: null }, project_path: "/Developer/codeterm", team_id: null, tasks: [] };
const index: PlanIndexEntry = { plan_id: plan.id, file_path: `/plans/${plan.id}.json`, owner: plan.owner, title: plan.title, status: "active", updated_at: 1, team_id: null, assigned_tab_ids: [], task_count: 0 };
provideList("list_plans_full", [plan]);
provideList("list_plan_index", [index]);

function cron(id: string, name: string, tab: string, waiting = false): CronJob {
  return {
    id, name, description: null, workspace_id: PHONE_WORKSPACE, team_id: null, schedule: "0 9 * * *", enabled: true, agent: "claude", model: null,
    reasoning_effort: null, task: name, cwd: null, role: null, creator_tab_id: tab, report_target_tab_id: null, oneshot: false, max_history: 10,
    created_at: 1, next_run: Date.now() + 40 * MINUTE, jitter_max_ms: null,
    last_run: waiting ? { run_id: `run-${id}`, started_at: 1, finished_at: null, status: "AwaitingOwnerPrompt", tab_id: tab } : null,
  };
}

provideList("cron_list", [
  cron("phone-digest", "Morning digest", ids.planned),
  cron("phone-triage", "Inbox triage", ids.planned, true),
  cron("phone-standup", "Standup notes", ids.crons),
  cron("phone-backup", "Nightly backup", ids.crons),
]);

const blueprint: BlueprintSnapshot = {
  id: "phone-train", name: "Nightly release train", owner: { kind: "tab", tab_id: ids.planned, team_id: null }, state: "running", run_id: "run-phone-train",
  current_step: 0, last_event_at_ms: 100, trigger: { type: "schedule", summary: "Every night at 01:00" },
  steps: [{ index: 0, name: "Build", kind: "command", title: "Build", state: "running", outcome: null, started_at_ms: 50, ended_at_ms: null, result_summary: null, reason: null }],
  result: { target: { kind: "owner_tab", id: ids.planned }, state: "waiting", reason: null, body_ref: null },
};
const graph: AutomationGraph = { workspace_id: PHONE_WORKSPACE, team_id: null, all_teams: true, revision: "gallery", scenarios: [], nodes: [], edges: [], blueprints: [blueprint] };
provideForWorkspace("automation_graph_workspace_snapshot", PHONE_WORKSPACE, graph);

function delayed(id: string, text: string, minutes: number | null): DelayedMessage {
  return { id, tabId: ids.planned, text, scheduledAtMs: Date.now() + (minutes ?? 15) * MINUTE, createdAtMs: 1, state: minutes == null ? "stashed" : "queued", sessionChanged: false } as DelayedMessage;
}

provideForTab("delayed_send_list", ids.planned, [
  delayed("phone-d1", "Check whether the release build finished and post the summary", 12),
  delayed("phone-d2", "Draft the changelog once QA signs off", null),
]);

const working = { state: "working" as const, status_line: "Polishing the phone composer… (2m 41s · ↓ 6.4k tokens)" };
const planned = agentTab(ids.planned, { title: "Phone composer", workspace_id: PHONE_WORKSPACE, agent_state: working });
const cronsOnly = agentTab(ids.crons, { title: "Inbox", workspace_id: PHONE_WORKSPACE });

function PhoneComposer(props: ChatBottomBarProps) {
  return <PhoneViewport><ChatBottomBar workspaceId={PHONE_WORKSPACE} {...props} /></PhoneViewport>;
}

export default defineSpecimen({
  id: SPECIMEN_ID,
  title: "Phone composer",
  category: "Composer",
  where: "src/components/Terminal/ComposerContextHeader.tsx · src/components/Terminal/AgentStateIndicator.tsx",
  stage: "wide",
  notes: "The real ChatComposer bottom bar in its NarrowExpand phone variant: a glass context pill floats above the capsule, unfolds the header with labelled kind pills, and Stop floats on the right. usePhoneViewport is forced by answering the product's SHELL_NARROW matchMedia query on this route only, and the phone-width @media rules are mirrored under the stage wrapper. Min-width rules (Tailwind sm:) still see the desktop window.",
  Component: PhoneComposer,
  states: [
    { id: "folded", label: "Working · pill folded · Stop floating", props: { tab: planned, status: "Working" } },
    { id: "unfolded", label: "Pill unfolded · labelled kind pills", props: { tab: planned, status: "Working", unfold: true } },
    { id: "unfolded-crons", label: "Unfolded · crons strip open", props: { tab: planned, status: "Working", unfold: true, openKind: "crons" } },
    { id: "unfolded-delayed", label: "Unfolded · delayed strip open", props: { tab: planned, status: "Working", unfold: true, openKind: "delayed" } },
    { id: "idle-crons", label: "Idle · crons only · no Stop", props: { tab: cronsOnly } },
  ],
});
