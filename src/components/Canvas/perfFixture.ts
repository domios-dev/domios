import { TaskPriority, TaskStatus, type Plan } from "./types";
import { milestone, plan, section, task } from "./layout/fixtures";

// Dev harness, LOCAL-ONLY: to validate rendering against a real exported plan,
// a dev stores the plan JSON in localStorage["canvas.realPlanJson"] and flips
// localStorage["canvas.realPlan"]="1". No real plan data is committed — the JSON
// lives only in the browser at dev time. Off (and side-effect-free) unless both set.
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

// Scale harness: localStorage "canvas.perfFixtureN" injects a synthetic N-task
// plan client-side so the canvas can be exercised at scale without the daemon
// ingestion path. Off (0) unless the flag is explicitly set — safe in any build.
export function perfFixtureCount(): number {
  if (typeof window === "undefined") return 0;
  try {
    const n = Number(window.localStorage.getItem("canvas.perfFixtureN") ?? "0");
    return Number.isFinite(n) && n > 0 ? Math.floor(n) : 0;
  } catch {
    return 0;
  }
}

const MS_KINDS = ["status_update", "testing", "review"];
const PRIORITIES = [TaskPriority.P0, TaskPriority.P1, TaskPriority.P2, TaskPriority.P3] as const;
const ROLES = ["backend-dev", "frontend-dev", "code-reviewer", null, null];
const GROUPS = ["layout", "navigation", null, "isolation", null];
const CLEANUP_STATES = ["pending", "not_applicable", "done", "failed"];

// Complex cards (not bare placeholders): long title, multi-line body, and several
// milestone bubbles each — representative of real card rendering cost at scale.
// Field spread (priority/role/branch/peer/group) is synthetic-only so the new
// field-surfacing can be visually audited without a real plan that populates them.
export function buildPerfFixturePlans(n: number): Plan[] {
  const sec = section("perf_sec", [], "Synthetic perf section body for scale testing.");
  const tasks = Array.from({ length: n }, (_, i) => {
    const milestones = Array.from({ length: 4 }, (_, m) =>
      milestone(`perf_t${i}_m${m}`, MS_KINDS[m % MS_KINDS.length], null),
    );
    return task(`perf_t${i}`, ["perf_sec"], {
      status: i % 6 === 0 ? TaskStatus.InProgress : i % 9 === 0 ? TaskStatus.Done : TaskStatus.Pending,
      created_at: i,
      priority: PRIORITIES[i % PRIORITIES.length],
      group: GROUPS[i % GROUPS.length],
      taken_by_role: ROLES[i % ROLES.length],
      peer: i % 7 === 0 ? "claude-sonnet" : null,
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
      status_label: i % 3 === 0 ? "in review" : null,
      milestones,
    });
  });
  return [plan("perf_plan", "perf_orch", 1, [sec], tasks, {
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
