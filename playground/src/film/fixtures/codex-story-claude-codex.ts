import type { NormalizedChatMessage } from "@codeterm/chat-core";
import type { Task } from "../../../../src/components/Canvas/types";
import type { BoardTaskCardModel } from "../../../../src/components/Canvas/planView/boardModel";
import { agentTab, CLAUDE, CODEX } from "../../gallery/fixtures";
import { PLAN, PLAN_INDEX } from "./plan";
import { TEAM_ID, FILM_WORKSPACE } from "./team";

const people = [
  ["Viris", CLAUDE.id, "opus", "Claude Opus"],
  ["Orion", CLAUDE.id, "opus", "Claude Opus"],
  ["Pip", CODEX.id, "gpt-6.1-sol", "Codex GPT-6.1-Sol"],
  ["Helvetia", CODEX.id, "gpt-6.1-sol", "Codex GPT-6.1-Sol"],
  ["Titan", CODEX.id, "gpt-6.1-sol", "Codex GPT-6.1-Sol"],
  ["Cutter", CLAUDE.id, "sonnet", "Claude Sonnet"],
] as const;

export const STORY_MEMBERS = people.map(([title, provider, model, description], i) => agentTab(`film-${title.toLowerCase()}`, {
  title, description, agent_type: provider, agent_session_model: model,
  workspace_id: FILM_WORKSPACE.id, team_id: TEAM_ID,
  role: i === 0 ? "Manager" : "Worker", role_profile: i === 0 ? "orchestrator" : null,
  parent_tab_id: i === 0 ? null : "film-viris", can_spawn: i === 0,
  agent_state: { state: "working", status_line: description },
}));

const canvas = { x: null, y: null, collapsed: false };
const task = (id: string, key: string, name: string, person: string, branch: string): Task => ({
  id, key, name, type: "implement", status: "in_progress", model: null, model_rationale: null,
  status_label: null, section_ids: [], body: "", intent: null, parent_task_id: null, peer: null,
  assigned_tab_id: `film-${person.toLowerCase()}`, personality: person, canvas, milestones: [],
  lane: { tab_id: `film-${person.toLowerCase()}`, branch },
});

export const LANE_TASKS = [
  task("task_3p3xlz", "COD-1653", "Mesh, memory and brand", "Orion", "lane/pl-ecu0d6-task-3p3xlz-62fe80d2"),
  task("task_yzdhz6", "COD-1683", "Codex studio: playful shots", "Pip", "lane/pl-ecu0d6-task-yzdhz6-d8dbe9a7"),
  task("task_cvy6c0", "COD-1654", "Reels, capture and review", "Cutter", "lane/pl-ecu0d6-task-cvy6c0-c966e4d1"),
];

export const STORY_PLAN = { ...PLAN, id: "pl_ecu0d6", title: "Domios showcase film", tasks: LANE_TASKS };
export const STORY_INDEX = { ...PLAN_INDEX, plan_id: STORY_PLAN.id, title: STORY_PLAN.title, assigned_tab_ids: STORY_MEMBERS.map(m => m.id) };

export function taskCard(task: Task): BoardTaskCardModel {
  return {
    task, statusChipText: task.status === "done" ? "Done" : task.status === "in_review" ? "In review" : "In progress",
    presenceLabel: task.personality ?? null, historicalExecutorSummary: task.personality ?? null,
    roleLabel: "Worker", branch: task.lane?.branch ?? null, latestNote: null, workingOn: null,
    loopBadge: null, competeBadge: null, focusMatch: false, locator: null,
  };
}

export const REPORTS: { sender: string; message: NormalizedChatMessage }[] = [
  { sender: "Orion", message: { id: "cx-report-orion", type: "user", content: "TASK: Mesh and memory\nSTATUS: in_review\nCHANGES: Mesh, memory graph and brand scenes.\n\nReady for review." } },
  { sender: "Mira", message: { id: "cx-report-mira", type: "user", content: "TASK: Product scenes\nSTATUS: in_review\nCHANGES: Product scenes from real components.\n\nReady for review." } },
  { sender: "Cutter", message: { id: "cx-report-cutter", type: "user", content: "TASK: Reels and capture\nSTATUS: in_review\nCHANGES: Seven hard-cut reels.\nTESTS: 22 clips pass the luma check.\n\nReady for review." } },
];

export const CUTTER_MERGE = "bbd585fb23";
export const CUTTER_COMMIT = "46252d072";
