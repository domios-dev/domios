import { TaskPriority, TaskStatus, type Plan, type Section } from "./types";
import type { Pane } from "../../lib/types";
import { milestone, plan, task } from "./layout/fixtures";
import {
  PERF_PLAN_SECTIONS,
  PERF_PLAN_SECTION_IDS,
  PERF_SPEC_ID,
  PERF_SPEC_LINKS,
  PERF_SPEC_SECTIONS,
} from "./perfFixtureSections";
import { buildCardMetadataFixturePlans } from "./cardMetadataFixture";

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

export function perfFixtureCount(): number {
  if (typeof window === "undefined") return 0;
  try {
    if (window.localStorage.getItem("canvas.perfFixtureProfile") === "done-heavy") return 210;
    if (window.localStorage.getItem("canvas.perfFixtureProfile") === "card-metadata") return 1;
    const n = Number(window.localStorage.getItem("canvas.perfFixtureN") ?? "0");
    return Number.isFinite(n) && n > 0 ? Math.floor(n) : 0;
  } catch {
    return 0;
  }
}

export function perfFixtureProfile(): "default" | "done-heavy" | "card-metadata" {
  if (typeof window === "undefined") return "default";
  try {
    const profile = window.localStorage.getItem("canvas.perfFixtureProfile");
    if (profile === "done-heavy") return "done-heavy";
    if (profile === "card-metadata") return "card-metadata";
    return "default";
  } catch {
    return "default";
  }
}

export { buildCardMetadataFixturePlans, buildCardMetadataFixturePanes } from "./cardMetadataFixture";

export function buildDoneHeavyPerfFixturePlans(): Plan[] {
  const sectionIds = PERF_PLAN_SECTION_IDS;
  const active = Array.from({ length: 10 }, (_, i) =>
    task(`perf_active_${i}`, [sectionIds[i % sectionIds.length]], {
      status: TaskStatus.InProgress,
      created_at: i,
      priority: PRIORITIES[i % PRIORITIES.length],
      assigned_pane_id: `pane-active-${i}`,
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
      assigned_pane_id: active ? `pane-agent-${i}` : null,
      handle: `t${i}-implement-feature-module-${i}`,
      lane: i % 2 === 0
        ? {
            pane_id: `pane-${i}`,
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

export function buildPerfFixturePanes(plans: Plan[]): Record<string, Pane> {
  const panes: Record<string, Pane> = {};
  const add = (id: string | null | undefined, title: string) => {
    const pid = id?.trim();
    if (pid && !panes[pid]) panes[pid] = { id: pid, pty_id: null, pane_type: "Terminal", title, cwd: null };
  };
  for (const p of plans) {
    for (const t of p.tasks) {
      add(t.assigned_pane_id, t.taken_by_role ?? t.name ?? "agent");
      add(t.lane?.pane_id, t.lane?.workspace_name ?? t.taken_by_role ?? "agent");
    }
  }
  return panes;
}

export function fixtureSpecSectionsByPlan(plans: Plan[]): Map<string, Section[]> {
  const out = new Map<string, Section[]>();
  for (const p of plans) {
    if (p.primary_spec_id === PERF_SPEC_ID) out.set(p.id, PERF_SPEC_SECTIONS);
  }
  return out;
}
