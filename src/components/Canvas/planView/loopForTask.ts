import type { LoopSpec, Plan, Task } from "../types";

/** Mirrors src-tauri/src/plan/loop_engine.rs STAGNATION_THRESHOLD. */
export const LOOP_STAGNATION_THRESHOLD = 2;

/** Minimum aggregated iteration count before the loop badge surfaces. */
export const LOOP_BADGE_MIN_COUNT = 2;

const TERMINAL_LOOP_STATUSES = new Set<LoopSpec["status"]>(["done", "exhausted", "stopped"]);

export interface LoopForTaskResult {
  count: number;
  budget: number;
  status: LoopSpec["status"];
  stopWhen: string | null;
  warning: boolean;
  tooltip: string;
  loops: LoopSpec[];
}

function clean(value?: string | null): string | null {
  const text = value?.trim() ?? "";
  return text || null;
}

export function findTaskInPlan(plan: Plan, taskId: string): Task | undefined {
  const needle = taskId.trim();
  return plan.tasks.find((task) => task.id === needle || task.handle?.trim() === needle);
}

export function loopTargetsTask(loop: LoopSpec, task: Task): boolean {
  const target = clean(loop.target);
  if (!target) return false;
  return target === task.id || target === clean(task.handle);
}

export function isActiveLoop(loop: LoopSpec): boolean {
  return loop.status === "running" && !TERMINAL_LOOP_STATUSES.has(loop.status);
}

export function isLoopStagnating(loop: LoopSpec): boolean {
  return (loop.stagnation ?? 0) >= LOOP_STAGNATION_THRESHOLD;
}

function loopStopWhen(loop: LoopSpec): string {
  return clean(loop.stop_when) ?? "(none recorded)";
}

function loopTooltipLine(loop: LoopSpec): string {
  const stagnating = isLoopStagnating(loop) ? " · stagnating" : "";
  return `${loop.id}: iteration ${loop.count}/${loop.budget} · ${loop.status} · stop_when: ${loopStopWhen(loop)}${stagnating}`;
}

export function loopForTask(plan: Plan, taskId: string): LoopForTaskResult | null {
  const task = findTaskInPlan(plan, taskId);
  if (!task) return null;

  const loops = (plan.loops ?? []).filter((loop) => isActiveLoop(loop) && loopTargetsTask(loop, task));
  if (!loops.length) return null;

  const count = loops.reduce((sum, loop) => sum + loop.count, 0);
  if (count < LOOP_BADGE_MIN_COUNT) return null;

  const warning = loops.some((loop) => loop.count >= loop.budget || isLoopStagnating(loop));
  const budget = Math.max(...loops.map((loop) => loop.budget));
  const stopWhen = loops.length === 1 ? loops[0].stop_when : null;
  const tooltip = loops.map(loopTooltipLine).join("\n");

  return {
    count,
    budget,
    status: "running",
    stopWhen,
    warning,
    tooltip,
    loops,
  };
}
