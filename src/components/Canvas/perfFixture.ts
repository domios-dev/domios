import { TaskPriority, TaskStatus, type Plan, type Section } from "./types";
import type { Tab } from "../../lib/types";
import { milestone, plan, task } from "./layout/fixtures";
import {
  PERF_PLAN_SECTIONS,
  PERF_PLAN_SECTION_IDS,
  PERF_SPEC_ID,
  PERF_SPEC_LINKS,
  PERF_SPEC_SECTIONS,
} from "./perfFixtureSections";

export function realFixtureEnabled(): boolean {
  if (typeof window === "undefined") return false;
  try {
    return window.localStorage.getItem("canvas.realPlan") === "1" && !!window.localStorage.getItem("canvas.realPlanJson");
  } catch {
    return false;
  }
}

export function realFixturePlans(): Plan[] {
  try {
    const raw = window.localStorage.getItem("canvas.realPlanJson");
    if (!raw) return [];
    const parsed = JSON.parse(raw) as Plan | Plan[];
    return Array.isArray(parsed) ? parsed : [parsed];
  } catch {
    return [];
  }
}

export type PerfFixtureProfile =
  | "default"
  | "done-heavy"
  | "card-metadata"
  | "plan-with-known-tasks"
  | "tab-tree-with-statuses"
  | "mid-stream-chat-transcript";

export function perfFixtureCount(): number {
  if (typeof window === "undefined") return 0;
  try {
    const profile = window.localStorage.getItem("canvas.perfFixtureProfile");
    if (profile === "done-heavy") return 210;
    if (
      profile === "card-metadata" ||
      profile === "plan-with-known-tasks" ||
      profile === "tab-tree-with-statuses" ||
      profile === "mid-stream-chat-transcript"
    ) {
      return 1;
    }
    const n = Number(window.localStorage.getItem("canvas.perfFixtureN") ?? "0");
    return Number.isFinite(n) && n > 0 ? Math.floor(n) : 0;
  } catch {
    return 0;
  }
}

export function perfFixtureProfile(): PerfFixtureProfile {
  if (typeof window === "undefined") return "default";
  try {
    const profile = window.localStorage.getItem("canvas.perfFixtureProfile");
    if (profile === "done-heavy") return "done-heavy";
    if (profile === "card-metadata") return "card-metadata";
    if (profile === "plan-with-known-tasks") return "plan-with-known-tasks";
    if (profile === "tab-tree-with-statuses") return "tab-tree-with-statuses";
    if (profile === "mid-stream-chat-transcript") return "mid-stream-chat-transcript";
    return "default";
  } catch {
    return "default";
  }
}

export { buildCardMetadataFixturePlans, buildCardMetadataFixtureTabs } from "./cardMetadataFixture";

export function buildDoneHeavyPerfFixturePlans(): Plan[] {
  const sectionIds = PERF_PLAN_SECTION_IDS;
  const active = Array.from({ length: 10 }, (_, i) =>
    task(`perf_active_${i}`, [sectionIds[i % sectionIds.length]], {
      status: TaskStatus.InProgress,
      created_at: i,
      priority: PRIORITIES[i % PRIORITIES.length],
      assigned_tab_id: `tab-active-${i}`,
      name: `Active task ${i}: current implementation lane`,
      body: `Active work item ${i} for canvas perf / legibility validation.`,
      milestones: [milestone(`perf_active_${i}_m0`, "status_update", null)],
    }),
  );
  const done = Array.from({ length: 200 }, (_, i) =>
    task(`perf_done_${i}`, [sectionIds[i % sectionIds.length]], {
      status: TaskStatus.Done,
      created_at: 100 + i,
      priority: PRIORITIES[i % PRIORITIES.length],
      name: `Done task ${i}: shipped module ${i}`,
      body: `Completed task ${i} — compressed on canvas at normal zoom.`,
      milestones: [],
    }),
  );
  return [plan("perf_plan", "perf_orch", 1, PERF_PLAN_SECTIONS, [...active, ...done], {
    primary_spec_id: PERF_SPEC_ID,
    spec_links: PERF_SPEC_LINKS,
    goal: {
      vector: "Done-heavy canvas legibility: active tasks dominate; done history compresses per section.",
      postulates: ["Overview LOD replaces dot-clouds with readable section chips"],
    },
    heartbeat: {
      current_focus: "Validate 200 done + 10 active under pan/zoom frame budget.",
      active_lanes: ["lane/canvas-legibility-lod"],
      rules: ["Frame probe p95 under ~16ms on dev hardware"],
      learnings: [],
    },
    loops: [],
  })];
}

export function buildPlanWithKnownTasksFixturePlans(): Plan[] {
  const [setup, implement, verify, docs] = PERF_PLAN_SECTION_IDS;
  const tasks = [
    task("known_setup", [setup], {
      status: TaskStatus.Done,
      created_at: 1,
      priority: TaskPriority.P1,
      name: "Seed deterministic workspace state",
      body: "Known completed setup task used by Playwright assertions.",
      milestones: [milestone("known_setup_m0", "testing", null)],
    }),
    task("known_impl", [implement], {
      status: TaskStatus.InProgress,
      created_at: 2,
      priority: TaskPriority.P0,
      assigned_tab_id: "tab-known-impl",
      taken_by_role: "frontend-dev",
      name: "Implement fixture-backed plan controls",
      body: "Stable in-progress task with a bound Tab and predictable title.",
      acceptance_criteria: ["Board card opens detail", "List row preserves scroll"],
      milestones: [milestone("known_impl_m0", "status_update", null)],
    }),
    task("known_review", [verify], {
      status: TaskStatus.InReview,
      created_at: 3,
      priority: TaskPriority.P2,
      assigned_tab_id: "tab-known-review",
      taken_by_role: "code-reviewer",
      name: "Review deterministic fixture migration",
      body: "Review task exercises non-pending status columns.",
      milestones: [milestone("known_review_m0", "review", null)],
    }),
    task("known_docs", [docs], {
      status: TaskStatus.Pending,
      created_at: 4,
      priority: TaskPriority.P3,
      name: "Document seeded world states",
      body: "Pending documentation task with stable copy.",
      milestones: [],
    }),
  ];
  return [plan("fixture_known_plan", "fixture_orch", 1, PERF_PLAN_SECTIONS, tasks, {
    primary_spec_id: PERF_SPEC_ID,
    spec_links: PERF_SPEC_LINKS,
    task_groups: [
      { key: "implementation", label: "Implementation", color: null },
      { key: "verification", label: "Verification", color: null },
    ],
    goal: {
      vector: "Known task plan for deterministic board/list/canvas Playwright tests.",
      postulates: ["Task ids, statuses, and Tab bindings remain stable across runs"],
    },
    heartbeat: {
      current_focus: "Migrating canvas tests away from live setup.",
      active_lanes: ["fixtures/known-tasks"],
      rules: ["No network-dependent plan data"],
      learnings: [],
    },
    loops: [{ id: "known_loop", target: "fixture migration", stages: [], budget: 3, count: 1, stop_when: null, status: "running" }],
  })];
}

export function buildTabTreeWithStatusesFixturePlans(): Plan[] {
  const plans = buildPlanWithKnownTasksFixturePlans();
  const p = plans[0];
  return [{
    ...p,
    id: "fixture_tab_tree_plan",
    title: "Tab tree with statuses",
    tasks: p.tasks.map((t, i) => ({
      ...t,
      assigned_tab_id: ["tab-orch", "tab-worker-active", "tab-worker-waiting", "tab-worker-idle"][i] ?? t.assigned_tab_id,
      parent_task_id: i > 1 ? "known_impl" : t.parent_task_id,
    })),
    heartbeat: {
      current_focus: "Validate Tab status badges and assigned-agent affordances.",
      active_lanes: ["tab-orch", "tab-worker-active", "tab-worker-waiting"],
      rules: ["Working, Waiting, Idle, and Dead Tabs are represented"],
      learnings: [],
    },
  }];
}

export function buildMidStreamChatTranscriptFixturePlans(): Plan[] {
  const plans = buildPlanWithKnownTasksFixturePlans();
  const p = plans[0];
  return [{
    ...p,
    id: "fixture_mid_stream_chat_plan",
    title: "Mid-stream chat transcript",
    tasks: p.tasks.map((t, i) => ({
      ...t,
      status: i === 0 ? TaskStatus.Done : i === 1 ? TaskStatus.InProgress : TaskStatus.Pending,
      assigned_tab_id: i === 1 ? "tab-chat-streaming" : t.assigned_tab_id,
      status_label: i === 1 ? "streaming reply" : t.status_label,
      name: i === 1 ? "Assistant reply is mid-stream" : t.name,
      body: i === 1
        ? "Fixture represents a chat Tab with user prompt, partial assistant response, and tool output in flight."
        : t.body,
    })),
    heartbeat: {
      current_focus: "Assistant is streaming a response while canvas remains usable.",
      active_lanes: ["tab-chat-streaming"],
      rules: ["Transcript state is deterministic"],
      learnings: [{ id: "chat_stream_1", text: "Partial replies should not shift plan controls.", ts: 1, tag: "chat" }],
    },
  }];
}

const MS_KINDS = ["status_update", "testing", "review"];
const PRIORITIES = [TaskPriority.P0, TaskPriority.P1, TaskPriority.P2, TaskPriority.P3] as const;
const ROLES = ["backend-dev", "frontend-dev", "code-reviewer", "docs-sync", null];
const GROUPS = ["layout", "navigation", null, "isolation", null];
const CLEANUP_STATES = ["pending", "not_applicable", "done", "failed"];
function syntheticPeer(i: number): string | null {
  if (i % 12 === 0) return "claude-sonnet, gpt-5-codex, gemini-2";
  if (i % 6 === 0) return "claude-sonnet";
  if (i % 7 === 0) return "opus-orchestrator";
  return null;
}

export function buildPerfFixturePlans(n: number): Plan[] {
  const sectionIds = PERF_PLAN_SECTION_IDS;
  const tasks = Array.from({ length: n }, (_, i) => {
    const milestones = Array.from({ length: 4 }, (_, m) =>
      milestone(`perf_t${i}_m${m}`, MS_KINDS[m % MS_KINDS.length], null),
    );
    const active = i % 6 === 0;
    // coverage (its glyph + count) while the document body stays readable.
    return task(`perf_t${i}`, [sectionIds[i % sectionIds.length]], {
      status: active ? TaskStatus.InProgress : i % 9 === 0 ? TaskStatus.Done : TaskStatus.Pending,
      created_at: i,
      priority: PRIORITIES[i % PRIORITIES.length],
      group: GROUPS[i % GROUPS.length],
      taken_by_role: ROLES[i % ROLES.length],
      peer: syntheticPeer(i),
      assigned_tab_id: active ? `tab-agent-${i}` : null,
      handle: `t${i}-implement-feature-module-${i}`,
      lane: i % 2 === 0
        ? {
            tab_id: `tab-${i}`,
            workspace_id: `ws-${i}`,
            workspace_name: i % 4 === 0 ? "codeterm" : `lane-workspace-${i}`,
            worktree_path: `/Users/dev/Developer/codeterm-module-${i}`,
            branch: `feat/module-${i}`,
            base: "master",
            destination: i % 4 === 0 ? "develop" : "master",
            cleanup_status: CLEANUP_STATES[i % CLEANUP_STATES.length],
            cleanup_hints: i % 6 === 0 ? ["Remove worktree and branch after orchestrator accepts the report."] : [],
          }
        : undefined,
      name: `Synthetic task ${i}: implement feature module ${i} with a deliberately long title to exercise wrapping and overflow`,
      body: `Description for task ${i}. Multi-line body to exercise text layout, wrapping, ellipsis and overflow within the card bounds. Acceptance: renders cleanly with title, body and milestone bubbles, no text spilling past the node frame.`,
      acceptance_criteria: [`Module ${i} renders schematic on the map`, `Full detail opens on expand`],
      test_plan: "tsc + canvas unit tests + multi-scale screenshots",
      correctness: `Map (schematic) and Read (detail) stay cleanly separated for module ${i}; behavior preserved.`,
      links: [`https://example.com/specs/module-${i}`, `PR #${100 + i}`],
      status_label: i % 3 === 0 ? "in review" : null,
      milestones,
    });
  });
  // gets 2-3 children with realistic short titles, swarming their parent tile.
  const SUBTASK_TITLES = ["wire the data layer", "add the failing test first", "render + a11y pass", "handle the empty + error states"];
  const subtaskParents = tasks.filter((_, i) => i % 5 === 0); // spread satellites across the cloud
  const subtasks = subtaskParents.flatMap((parent, pi) =>
    Array.from({ length: 2 + (pi % 2) }, (_, ci) =>
      task(`${parent.id}_sub${ci}`, parent.section_ids, {
        parent_task_id: parent.id,
        status: ci === 0 ? TaskStatus.InProgress : ci === 1 ? TaskStatus.Pending : TaskStatus.Done,
        created_at: 1000 + pi * 10 + ci,
        priority: PRIORITIES[(pi + ci) % PRIORITIES.length],
        name: `Subtask ${ci + 1}: ${SUBTASK_TITLES[(pi + ci) % SUBTASK_TITLES.length]}`,
        body: `Subtask ${ci + 1} of ${parent.id}.`,
        milestones: [],
      }),
    ),
  );
  return [plan("perf_plan", "perf_orch", 1, PERF_PLAN_SECTIONS, [...tasks, ...subtasks], {
    primary_spec_id: PERF_SPEC_ID,
    spec_links: PERF_SPEC_LINKS,
    task_groups: [
      { key: "layout", label: "Layout", color: null },
      { key: "navigation", label: "Navigation", color: null },
      { key: "isolation", label: "Isolation", color: null },
    ],
    goal: {
      vector: "Render every plan at any scale with crisp text, zero reflow and full field coverage — the canvas is the orchestrator's command center.",
      postulates: ["Object size is the camera matrix, never a per-card re-rasterization hack"],
    },
    heartbeat: {
      current_focus: "Packing the task cloud flush against the plan and surfacing the orchestrator heartbeat + loop status on the plan node.",
      active_lanes: ["feat/canvas-perf-polish", "feat/heartbeat-surface", "feat/loop-status"],
      rules: ["Keep tsc + cloudFlush green", "No per-frame O(N) work", "Render fields only when present"],
      learnings: [
        { id: "hl1", text: "appW band beats raw camera scale for per-node LOD.", ts: 1, tag: "perf" },
        { id: "hl2", text: "Counter-scale re-raster keeps glyphs crisp at deep zoom.", ts: 2, tag: "render" },
      ],
    },
    loops: [
      { id: "lp1", target: "canvas card field coverage", stages: [{ key: "audit", role: "designer", model: null, why: null, prompt: null }, { key: "fix", role: "frontend-dev", model: null, why: null, prompt: null }, { key: "verify", role: "reviewer", model: null, why: null, prompt: null }], budget: 10, count: 2, stop_when: "every field surfaced + zero overflow", status: "running" },
      { id: "lp2", target: "perf regression sweep", stages: [], budget: 5, count: 5, stop_when: null, status: "done" },
      { id: "lp3", target: "spec drift reconcile", stages: [], budget: 3, count: 3, stop_when: null, status: "exhausted" },
    ],
  })];
}

export function buildPerfFixtureTabs(plans: Plan[]): Record<string, Tab> {
  const tabs: Record<string, Tab> = {};
  const add = (id: string | null | undefined, title: string) => {
    const pid = id?.trim();
    if (pid && !tabs[pid]) tabs[pid] = { id: pid, pty_id: null, tab_type: "Terminal", title, cwd: null };
  };
  for (const p of plans) {
    for (const t of p.tasks) {
      add(t.assigned_tab_id, t.taken_by_role ?? t.name ?? "agent");
      add(t.lane?.tab_id, t.lane?.workspace_name ?? t.taken_by_role ?? "agent");
    }
  }
  return tabs;
}

export function buildTabTreeWithStatusesFixtureTabs(): Record<string, Tab> {
  return {
    "tab-orch": { id: "tab-orch", pty_id: null, tab_type: "Terminal", title: "Orchestrator", cwd: null, agent_type: "codex", can_spawn: true, depth: 0, agent_state: { state: "working", status_line: "Coordinating fixture migration", elapsed_ms: 42000 } },
    "tab-worker-active": { id: "tab-worker-active", pty_id: null, tab_type: "Terminal", title: "Fisher", cwd: null, agent_type: "codex", parent_tab_id: "tab-orch", depth: 1, agent_state: { state: "working", status_line: "Running assertions", elapsed_ms: 18000 } },
    "tab-worker-waiting": { id: "tab-worker-waiting", pty_id: null, tab_type: "Terminal", title: "Reviewer", cwd: null, agent_type: "codex", parent_tab_id: "tab-orch", depth: 1, agent_state: { state: "question", status_line: "Waiting for report", elapsed_ms: 9000 } },
    "tab-worker-idle": { id: "tab-worker-idle", pty_id: null, tab_type: "Terminal", title: "Docs", cwd: null, agent_type: "codex", parent_tab_id: "tab-orch", depth: 1, agent_state: { state: "idle", status_line: "Idle", elapsed_ms: 0 } },
  };
}

export function buildMidStreamChatTranscriptFixtureTabs(): Record<string, Tab> {
  return {
    "tab-chat-streaming": {
      id: "tab-chat-streaming",
      pty_id: null,
      tab_type: "Terminal",
      title: "General Agent · streaming fixture",
      cwd: null,
      agent_type: "codex",
      chat_view: true,
      agent_state: { state: "working", status_line: "assistant reply streaming", elapsed_ms: 6500 },
      pending_prompt: {
        question: "Summarize the fixture migration and keep the canvas responsive.",
        options: [],
        submit_available: true,
      },
    },
    "tab-known-impl": { id: "tab-known-impl", pty_id: null, tab_type: "Terminal", title: "Implementation lane", cwd: null, agent_type: "codex", agent_state: { state: "working", status_line: "Applying patch", elapsed_ms: 12000 } },
    "tab-known-review": { id: "tab-known-review", pty_id: null, tab_type: "Terminal", title: "Review lane", cwd: null, agent_type: "codex", agent_state: { state: "question", status_line: "Awaiting diff", elapsed_ms: 3000 } },
  };
}

export function fixtureSpecSectionsByPlan(plans: Plan[]): Map<string, Section[]> {
  const out = new Map<string, Section[]>();
  for (const p of plans) {
    if (p.primary_spec_id === PERF_SPEC_ID) out.set(p.id, PERF_SPEC_SECTIONS);
  }
  return out;
}
