import type { Plan } from "./types";
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

// Complex cards (not bare placeholders): long title, multi-line body, and several
// milestone bubbles each — representative of real card rendering cost at scale.
export function buildPerfFixturePlans(n: number): Plan[] {
  const sec = section("perf_sec", [], "Synthetic perf section body for scale testing.");
  const tasks = Array.from({ length: n }, (_, i) => {
    const milestones = Array.from({ length: 4 }, (_, m) =>
      milestone(`perf_t${i}_m${m}`, MS_KINDS[m % MS_KINDS.length], null),
    );
    return task(`perf_t${i}`, ["perf_sec"], {
      status: i % 6 === 0 ? "in_progress" : i % 9 === 0 ? "done" : "pending",
      created_at: i,
      name: `Synthetic task ${i}: implement feature module ${i} with a deliberately long title to exercise wrapping and overflow`,
      body: `Description for task ${i}. Multi-line body to exercise text layout, wrapping, ellipsis and overflow within the card bounds. Acceptance: renders cleanly with title, body and milestone bubbles, no text spilling past the node frame.`,
      status_label: i % 3 === 0 ? "in review" : null,
      milestones,
    });
  });
  return [plan("perf_plan", "perf_orch", 1, [sec], tasks)];
}
