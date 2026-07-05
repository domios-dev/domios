import { TaskStatus } from "./types";
import type { Plan } from "./types";
import { milestone, plan, task } from "./layout/fixtures";
import {
  PERF_PLAN_SECTION_IDS,
  PERF_PLAN_SECTIONS,
  PERF_SPEC_ID,
  PERF_SPEC_LINKS,
} from "./perfFixtureSections";

const NOW = 1_700_000_000_000;
const SEC = PERF_PLAN_SECTION_IDS[0] ?? "pl_layout";
const LONG_BRANCH = "lane/task-73ea1b9d-card-metadata";

export const CARD_METADATA_TASK_IDS = {
  pending: "meta_pending",
  live: "meta_live",
  noPane: "meta_no_pane",
  paneGone: "meta_pane_gone",
  branchMissing: "meta_branch_miss",
  blocked: "meta_blocked",
  done: "meta_done",
  longBranch: "meta_long_branch",
} as const;

export function buildCardMetadataFixturePlans(): Plan[] {
  const doneMilestone = {
    ...milestone("meta_done_ms", "status_update", null, []),
    from: { pane_id: "pane-verdi", session_id: null, title: "Verdi" },
    text: "Shipped metadata album",
    ts: NOW - 60_000,
  };

  const tasks = [
    task(CARD_METADATA_TASK_IDS.pending, [SEC], {
      name: "Pending handle row",
      handle: "card-metadata-pending",
      status: TaskStatus.Pending,
    }),
    task(CARD_METADATA_TASK_IDS.live, [SEC], {
      name: "Live worker",
      status: TaskStatus.InProgress,
      model: "gpt-5.5",
      assigned_pane_id: "pane-verdi",
      lane: { branch: "lane/card-metadata-live" },
      status_changed_at: NOW - 120_000,
    }),
    task(CARD_METADATA_TASK_IDS.longBranch, [SEC], {
      name: "Long branch truncation",
      status: TaskStatus.InProgress,
      model: "claude:opus",
      assigned_pane_id: "pane-verdi",
      lane: { branch: LONG_BRANCH },
      status_changed_at: NOW - 90_000,
    }),
    task(CARD_METADATA_TASK_IDS.noPane, [SEC], {
      name: "No pane recorded",
      status: TaskStatus.InProgress,
      model: "gpt-5.5",
      assigned_pane_id: null,
      lane: { branch: "lane/no-pane" },
      status_changed_at: NOW - 30_000,
    }),
    task(CARD_METADATA_TASK_IDS.paneGone, [SEC], {
      name: "Pane gone",
      status: TaskStatus.InReview,
      assigned_pane_id: "ghost-pane",
      peer: null,
      lane: { branch: "lane/pane-gone" },
      status_changed_at: NOW - 45_000,
    }),
    task(CARD_METADATA_TASK_IDS.branchMissing, [SEC], {
      name: "Branch missing",
      status: TaskStatus.InProgress,
      assigned_pane_id: "pane-verdi",
      lane: {},
      status_changed_at: NOW - 20_000,
    }),
    task(CARD_METADATA_TASK_IDS.blocked, [SEC], {
      name: "Blocked with age",
      status: TaskStatus.Blocked,
      handle: "blocked-meta",
      lane: { branch: "lane/blocked-meta" },
      status_changed_at: NOW - 3_600_000,
    }),
    task(CARD_METADATA_TASK_IDS.done, [SEC], {
      name: "Done historical executor",
      status: TaskStatus.Done,
      assigned_pane_id: "pane-verdi",
      taken_by_role: "frontend-dev",
      lane: { branch: "lane/merged-meta" },
      status_changed_at: NOW - 86_400_000,
      milestones: [doneMilestone],
    }),
  ];

  return [
    plan("card_metadata_plan", "orch-meta", NOW, PERF_PLAN_SECTIONS, tasks, {
      primary_spec_id: PERF_SPEC_ID,
      spec_links: PERF_SPEC_LINKS,
      goal: { vector: "Task-card metadata screenshot album fixture" },
      heartbeat: { current_focus: `task:${CARD_METADATA_TASK_IDS.live}` },
    }),
  ];
}
