import type { Plan, Task } from "../../../../src/components/Canvas/types";
import type { Tab } from "../../../../src/lib/types";
import { MEMBERS } from "./team";
import { PLAN, PLAN_INDEX, PRODUCT_TASK } from "./plan";
import type { ChatRow } from "./fullui-chat";

const template = MEMBERS.find((tab) => tab.role_profile === "orchestrator")!;
const worker = MEMBERS.find((tab) => tab.role_profile === null)!;

export const STORY_MEMBERS: Tab[] = [
  { ...template, id: "film-viris", title: "Viris", description: "Orchestrator", agent_state: { state: "working", status_line: "Reviewing the film lanes" } },
  { ...worker, id: "toldit-dumas", title: "Dumas", role: "Manager", role_profile: "planner", parent_tab_id: null, description: "Planner", agent_state: { state: "idle", status_line: "Plan ready" } },
  ...["Mira", "Orion", "Cutter"].map((title): Tab => ({ ...worker, id: `toldit-${title.toLowerCase()}`, title, agent_session_model: title === "Cutter" ? "sonnet" : "opus", description: title === "Cutter" ? "Reels and capture" : title === "Mira" ? "Product scenes" : "Mesh and memory", agent_state: { state: "working", status_line: title === "Cutter" ? "Rendering the film" : "Framing the scenes" } })),
];

const taskFacts = [
  ["task_t6bol1", "COD-1652", "Product scenes from real components", "Mira", "lane/pl-ecu0d6-task-t6bol1-a25d227a"],
  ["task_3p3xlz", "COD-1653", "Mesh, memory graph and brand scenes", "Orion", "lane/pl-ecu0d6-task-3p3xlz-62fe80d2"],
  ["task_cvy6c0", "COD-1654", "Reels, batch capture and review sheets", "Cutter", "lane/pl-ecu0d6-task-cvy6c0-c966e4d1"],
];

export const STORY_TASKS: Task[] = taskFacts.map(([id, key, name, personality, branch]) => ({
  ...PRODUCT_TASK, id, key, name, personality, status: "in_progress", priority: undefined,
  executor_role: personality === "Cutter" ? "Film editor" : "Film frontend engineer", role_guidance: "", body: name,
  assigned_tab_id: `toldit-${personality.toLowerCase()}`, section_ids: [], milestones: [], acceptance_criteria: [], test_plan: "", correctness: "",
  lane: { branch, destination: "lane/viris-film" },
}));

export const STORY_PLAN: Plan = { ...PLAN, id: "pl_ecu0d6", title: "Domios showcase film (desktop)", sections: [], tasks: STORY_TASKS, strides: [], owner: { tab_id: "film-viris", session_id: null, title: "Viris" } };
export const STORY_INDEX = { ...PLAN_INDEX, plan_id: STORY_PLAN.id, title: STORY_PLAN.title, assigned_tab_ids: STORY_MEMBERS.map((tab) => tab.id), task_count: STORY_TASKS.length };
export const STORY_TABS = Object.fromEntries(STORY_MEMBERS.map((tab) => [tab.id, tab]));

export const OWNER_ROWS: ChatRow[] = [
  { at: -1, message: { id: "toldit-variants", type: "user", content: "Make five variants." } },
  { at: 0.8, message: { id: "toldit-real", type: "user", content: "Use the real interface." } },
  { at: 1.9, message: { id: "toldit-cuts", type: "user", content: "Cut the dissolves." } },
  { at: 3.0, message: { id: "toldit-answer", type: "assistant", content: "A film team. Separate lanes. Hard cuts." } },
];

export const REVIEW_ROWS: ChatRow[] = [
  { at: -1, message: { id: "toldit-bounce", type: "assistant", content: "**Bounce.** Mesh is the centrepiece and stays one block. Scene starts are too sparse." } },
  { at: 2.6, message: { id: "toldit-reworked", type: "user", inboundKind: "report", fromLabel: "Cutter", fromTab: "toldit-cutter", fromModel: "sonnet", content: "Mesh block kept together. Sparse heads trimmed. Reels recaptured." } },
  { at: 4.5, message: { id: "toldit-accepted", type: "assistant", content: "**Accept.** All cuts are hard, and luma passes on all 22 mp4s." } },
];

export const MERGE_ROWS: ChatRow[] = [{ at: -1, message: { id: "toldit-merge", type: "assistant", content: "Real-merged into **lane/viris-film**; the Orchestrator now mixes voice and music per reel." } }];
