import type { Task } from "../../../../src/components/Canvas/types";
import { MEMBERS } from "./team";
import { PLAN, PLAN_INDEX, PRODUCT_TASK } from "./plan";
import type { TimedMessage } from "../scenes/lab/labChat";

export const FLOW_MEMBERS = MEMBERS.filter((member) => member.role_profile !== "curator").map((member) => ({
  ...member,
  description: member.role_profile === "orchestrator" ? "Orchestrator" : member.role_profile === "planner" ? "Planner" : "Worker",
}));

export const FLOW_TASK: Task = {
  ...PRODUCT_TASK,
  priority: undefined,
  personality: "Mira",
  executor_role: "Sync engineer",
  role_guidance: "",
  section_ids: [],
  body: "Keep both offline edits.",
  acceptance_criteria: [],
  milestones: [],
  lane: { branch: "lane/offline-sync-resolver" },
};

export const FLOW_PLAN = { ...PLAN, tasks: [FLOW_TASK] };
export const FLOW_INDEX = { ...PLAN_INDEX, task_count: 1 };
export const FLOW_TABS = Object.fromEntries(FLOW_MEMBERS.map((member) => [member.id, member]));

export const REQUEST: TimedMessage[] = [
  { at: -1, message: { id: "mob-flow-owner", type: "user", content: "Can we make offline sync work?" } },
  { at: 1.6, message: { id: "mob-flow-reply", type: "assistant", content: "I'll put a team on it." } },
];

export const REPORT: TimedMessage[] = [
  { at: -1, message: { id: "mob-flow-lead", type: "assistant", content: "Mira is checking the resolver." } },
  { at: 1.4, message: { id: "mob-flow-report", type: "user", inboundKind: "report", fromLabel: "Mira", content: "Both edits survive.\n\n214 resolver tests pass. Ready for review." } },
];

export const MERGE: TimedMessage[] = [
  { at: -1, message: { id: "mob-flow-review", type: "user", inboundKind: "report", fromLabel: "Mira", content: "Ready for review." } },
  { at: 1.6, message: { id: "mob-flow-accept", type: "assistant", content: "Accepted. Merged `lane/offline-sync-resolver`." } },
];
