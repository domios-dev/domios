import { DetailContent, type DetailContentProps } from "../../../../src/components/Canvas/canvasRender/DetailContent";
import type { Milestone, Plan, Section, Task } from "../../../../src/components/Canvas/types";
import type { Tab } from "../../../../src/lib/types";
import { agentTab } from "../fixtures";
import { defineSpecimen } from "../types";

const MINUTE = 60_000;
const PLAN_ID = "plan-gallery";
const CANVAS = { x: null, y: null, collapsed: false };

const lead = agentTab("detail-lead", { title: "Release lead", role: "Manager", role_profile: "orchestrator" });
const designer = agentTab("detail-designer", { title: "Detail designer", agent_session_model: "opus" });
const TABS: Record<string, Tab> = { [lead.id]: lead, [designer.id]: designer };

function stride(id: string, kind: string, minutesAgo: number, text: string, author: Tab | null = designer): Milestone {
  return { id, from: { tab_id: author?.id ?? null, session_id: null, title: null }, kind, text, ts: Date.now() - minutesAgo * MINUTE, links: [], loop_id: null, iteration: null, canvas: CANVAS };
}

function task(id: string, key: string, name: string, overrides: Partial<Task> = {}): Task {
  return {
    id, key, name, type: "implement", model: null, model_rationale: null, status: "pending", status_label: null, section_ids: [], body: "",
    intent: null, parent_task_id: null, assigned_tab_id: null, peer: null, canvas: CANVAS, milestones: [], ...overrides,
  };
}

function section(id: string, title: string, body: string, subsections: Section[] = []): Section {
  return { id, title, body, subsections, canvas: CANVAS };
}

const SECTIONS = [
  section("s-detail", "Calm detail surfaces", "Task and plan detail read by importance: role first, then the agent, then the work."),
  section("s-activity", "One activity row", "Task activity and plan recent activity share the same log row."),
  section("s-ask", "Ask about a selection", "Select any text in a detail to ask the plan's orchestrator about it."),
];

const DETAIL_TASK = task("t-detail", "REL-12", "Task detail leads with the role", {
  status: "in_progress", priority: "p1", type: "implement", section_ids: ["s-detail", "s-activity"],
  executor_role: "Frontend engineer",
  role_guidance: "Own the **detail column** end to end. Keep the type rhythm even and never add a second chip row.",
  body: "Replace the chip soup with one agent card: provider logo, persona, model and the lane it works in.\n\n- Role card first\n- Clickable agent card opens the chat",
  acceptance_criteria: ["The role reads before the description", "The agent card opens the executor's chat", "No raw colours outside the tone tokens"],
  assigned_tab_id: designer.id,
  lane: { tab_id: designer.id, branch: "lane/detail-calm", worktree_path: "/Developer/codeterm-worktrees/detail-calm", workspace_name: "CodeTerm" },
  milestones: [
    stride("m1", "progress", 6, "Agent card renders the provider logo and model; lane rows moved under it."),
    stride("m2", "decision", 22, "Dropped the status chip row: the status lives in the header pill only."),
    stride("m3", "blocker", 48, "Waiting on the shared ActivityLogRow before the timeline can switch over.", lead),
    stride("m4", "progress", 75, "Role card with the executor role name and its guidance as markdown."),
  ],
});

const ROW_TASK = task("t-rows", "REL-13", "Shared activity log row", {
  status: "in_review", section_ids: ["s-activity"], assigned_tab_id: "detail-gone", personality: "Row smith",
  lane: { branch: "lane/activity-row", worktree_path: "/Developer/codeterm-worktrees/activity-row" },
  body: "One ActivityLogRow for the task timeline and the plan rail.",
  milestones: [stride("m5", "review", 12, "Ready for review: both callers use the row; expand and open variants covered.", null)],
});

const BARE_TASK = task("t-bare", "REL-14", "Selection ask popover");
const DONE_TASKS = [
  task("t-tokens", "REL-9", "Detail tones from tokens", { status: "done", section_ids: ["s-detail"] }),
  task("t-rhythm", "REL-10", "Even type rhythm", { status: "done", section_ids: ["s-detail"] }),
];

function plan(overrides: Partial<Plan> = {}): Plan {
  return {
    id: PLAN_ID, title: "Calm plan detail", sections: SECTIONS, status: "active", mode: "execution",
    owner: { tab_id: lead.id, session_id: null, title: null }, project_path: "/Developer/codeterm", created_at: 1, updated_at: Date.now(),
    milestone_kinds: [], loops: [], elements: [], canvas: { viewport: null }, owner_history: [],
    goal: { vector: "Every plan and task detail reads by importance, with one calm card per idea." },
    tasks: [DETAIL_TASK, ROW_TASK, BARE_TASK, ...DONE_TASKS],
    ...overrides,
  };
}

const PLAN = plan();
const PLANNING = plan({ mode: "planning", tasks: [], title: "Selection ask", goal: { vector: "Ask the orchestrator about any selected text without leaving the detail." } });
const COMPLETE = plan({
  mode: "complete",
  tasks: PLAN.tasks.map((entry) => ({ ...entry, status: "done" as const })),
  strides: [stride("retro", "retrospective", 5, "Shipped the calm detail in three passes. **Keep:** one shared log row. **Change:** start from the role card next time.", lead)],
});

const open = () => undefined;
const base: DetailContentProps = { kind: "task", planId: PLAN_ID, tabs: TABS, onOpenTab: open, onOpenTask: open, onSectionClick: open };

function DetailSpecimen(props: DetailContentProps) {
  return <div className="gallery-fill"><DetailContent {...props} /></div>;
}

export default defineSpecimen({
  id: "plan-task-detail",
  title: "Task and plan detail",
  category: "Plans",
  where: "src/components/Canvas/canvasRender/DetailContent.tsx",
  stage: "wide",
  notes: "The shipped detail body of the plan side panel. kind=task leads with the role card, then one agent card with the provider logo (alive tab opens its chat; a closed tab says so), description, acceptance check boxes, linked plan sections and the activity log on ActivityLogRow. kind=plan shows the goal, progress, every section as a calm card with one linked task plus +N more, and recent activity rows that open their task.",
  Component: DetailSpecimen,
  states: [
    { id: "task-live", label: "Task · role · live agent · criteria · activity", props: { ...base, task: DETAIL_TASK, plan: PLAN } },
    { id: "task-closed", label: "Task · persona on a closed tab · lane", props: { ...base, task: ROW_TASK, plan: PLAN } },
    { id: "task-bare", label: "Task · pending · no description", props: { ...base, task: BARE_TASK, plan: PLAN } },
    { id: "plan-execution", label: "Plan · goal · progress · +N more · recent activity", props: { ...base, kind: "plan", plan: PLAN } },
    { id: "plan-planning", label: "Plan · planning · no tasks yet", props: { ...base, kind: "plan", plan: PLANNING } },
    { id: "plan-complete", label: "Plan · complete · retrospective", props: { ...base, kind: "plan", plan: COMPLETE } },
  ],
});
