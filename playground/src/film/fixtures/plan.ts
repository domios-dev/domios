import type { PlanIndexEntry } from "../../../../src/components/Canvas/plansApi";
import type { Milestone, Plan, Section, Task, TaskLaneMetadata } from "../../../../src/components/Canvas/types";
import { ago, FILM_NOW } from "./clock";
import { FILM_WORKSPACE, MEMBERS, TEAM_ID } from "./team";

export const PLAN_ID = "pl_k4r8sn";
const CANVAS = { x: null, y: null, collapsed: false };
const [HELM, ATLAS, , MIRA, ORBIT, QUILL] = MEMBERS.map((member) => member.id);

function stride(id: string, kind: string, minutesAgo: number, text: string, authorId: string): Milestone {
  return { id, from: { tab_id: authorId, session_id: null, title: null }, kind, text, ts: ago(minutesAgo), links: [], loop_id: null, iteration: null, canvas: CANVAS };
}

function section(id: string, title: string, body: string): Section {
  return { id, title, body, subsections: [], canvas: CANVAS };
}

function lane(tabId: string, slug: string): TaskLaneMetadata {
  return { tab_id: tabId, branch: `lane/offline-sync-${slug}`, worktree_path: `C:/Developer/fieldnote-worktrees/offline-sync-${slug}`, workspace_name: FILM_WORKSPACE.name };
}

function task(id: string, key: string, name: string, status: Task["status"], overrides: Partial<Task> = {}): Task {
  return {
    id, key, name, type: "implement", model: null, model_rationale: null, status, status_label: null, section_ids: [], body: "",
    intent: null, parent_task_id: null, assigned_tab_id: null, peer: null, canvas: CANVAS, milestones: [], ...overrides,
  };
}

export const SECTIONS: Section[] = [
  section("sec-store", "Local store and schema", "Every note lives in SQLite first; schema v8 adds vector clocks and tombstones."),
  section("sec-sync", "Sync engine", "Edits queue while offline and replay in order; concurrent edits merge without losing either side."),
  section("sec-qa", "Sync QA", "A 48-scenario matrix on gpu-rig: airplane mode, clock skew, three devices editing one note."),
  section("sec-docs", "Docs and release", "The offline sync guide, migration notes and the 2.4 changelog."),
];

export const PRODUCT_TASK = task("task-resolver", "FN-203", "Conflict resolver for concurrent edits", "in_review", {
  priority: "p1", section_ids: ["sec-sync"], executor_role: "Sync engine engineer",
  role_guidance: "Merge **field by field** against the last synced base. Never drop an edit: when in doubt keep both and flag the note.",
  body: "Resolve edits made on several devices while they were offline.\n\n- Note bodies merge three-way\n- Checklist reorders keep the latest move per item",
  acceptance_criteria: [
    "Two devices editing one note offline both keep their edits",
    "Deleting a note another device edited restores it with the edit",
    "214 resolver tests pass on Windows, macOS and Linux",
  ],
  assigned_tab_id: MIRA,
  lane: lane(MIRA, "resolver"),
  milestones: [
    stride("m-proof", "review", 4, "Ready for review at 8c41e2f: 214 resolver tests passed, 0 failed; both edits survive on three devices.", MIRA),
    stride("m-delete", "progress", 19, "Delete-versus-edit now restores the note with the edit applied.", MIRA),
    stride("m-title", "decision", 41, "Titles keep the newest write; bodies always merge three-way.", HELM),
    stride("m-merge", "progress", 63, "Three-way body merge passes all 60 recorded conflicts.", MIRA),
  ],
});

export const TASKS: Task[] = [
  task("task-schema", "FN-201", "Schema migration v7 to v8: vector clocks and tombstones", "done", { section_ids: ["sec-store"], assigned_tab_id: MIRA, lane: lane(MIRA, "schema"),
    milestones: [stride("m-schema", "review", 140, "Merged at 3e7b0d2: migrates a 40k-note library in 1.8 s, 96 tests passed.", MIRA)] }),
  task("task-queue", "FN-202", "Offline edit queue with ordered replay", "done", { section_ids: ["sec-sync"], assigned_tab_id: MIRA, lane: lane(MIRA, "queue") }),
  PRODUCT_TASK,
  task("task-qa", "FN-204", "Sync QA matrix on gpu-rig", "in_progress", { section_ids: ["sec-qa"], assigned_tab_id: ORBIT, lane: lane(ORBIT, "qa") }),
  task("task-indicator", "FN-205", "Sync status badge in the editor", "done", { section_ids: ["sec-sync"], assigned_tab_id: QUILL, lane: lane(QUILL, "badge") }),
  task("task-docs", "FN-206", "Offline sync guide and migration notes", "in_progress", { section_ids: ["sec-docs"], assigned_tab_id: QUILL, lane: lane(QUILL, "docs") }),
  task("task-background", "FN-207", "Background sync on iOS and Android", "pending", { section_ids: ["sec-sync"] }),
  task("task-telemetry", "FN-208", "Conflict rate in the release dashboard", "pending", { section_ids: ["sec-qa"] }),
  task("task-changelog", "FN-209", "2.4 changelog and release tag", "blocked", { section_ids: ["sec-docs"], pause_reason: "Waits for the resolver and the QA matrix." }),
];

export const PLAN: Plan = {
  id: PLAN_ID, title: "Offline sync", sections: SECTIONS, status: "active", mode: "execution", team_id: TEAM_ID,
  owner: { tab_id: HELM, session_id: null, title: null }, project_path: "C:/Developer/fieldnote", created_at: ago(26 * 60), updated_at: FILM_NOW,
  milestone_kinds: [], loops: [], elements: [], canvas: { viewport: null }, owner_history: [],
  goal: { vector: "Fieldnote works fully offline: edits made on any device sync later and never overwrite each other." },
  tasks: TASKS,
  strides: [
    stride("s-merge", "progress", 7, "Merged lane/offline-sync-schema; the resolver and QA lanes branch from it.", HELM),
    stride("s-gate", "decision", 52, "2.4 ships when the resolver is merged, the QA matrix is 48 of 48 and the guide is live.", ATLAS),
  ],
};

export const PLAN_TABS = Object.fromEntries(MEMBERS.map((member) => [member.id, member]));

export const PLAN_INDEX: PlanIndexEntry = {
  plan_id: PLAN_ID, file_path: `C:/Developer/fieldnote/.domios/plans/active/${PLAN_ID}/plan.json`, canonical_project_path: "C:/Developer/fieldnote",
  owner: { tab_id: HELM, session_id: null, title: null }, title: PLAN.title, status: "active", updated_at: FILM_NOW, team_id: TEAM_ID,
  assigned_tab_ids: MEMBERS.map((member) => member.id), task_count: TASKS.length,
};
