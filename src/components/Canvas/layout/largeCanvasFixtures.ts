import type { Plan } from "../types";
import { layoutWorkspace } from "./index";
import { coreSolidNodes, layoutFingerprint, overlappingPairs, plan, section, task } from "./fixtures";
import type { WorkspaceLayout } from "./types";

export const LARGE_CANVAS_WORLD_SIZES = [100, 500, 1000] as const;
export type LargeCanvasWorldSize = (typeof LARGE_CANVAS_WORLD_SIZES)[number];

const SECTION_DEFS = [
  { id: "sec_core", body: "Core migration track" },
  { id: "sec_perf", body: "Performance and virtualization" },
  { id: "sec_overlay", body: "Rich overlay budget" },
  { id: "sec_lod", body: "Worlds LOD bands" },
  { id: "sec_focus", body: "Focus-by-id navigation" },
] as const;

export function largeCanvasPlan(taskCount: LargeCanvasWorldSize): Plan {
  const sections = SECTION_DEFS.map((s) => section(s.id, [], s.body));
  const secIds = sections.map((s) => s.id);
  const tasks = Array.from({ length: taskCount }, (_, i) => {
    const status = i % 11 === 0 ? "done" : i % 5 === 0 ? "in_progress" : i % 7 === 0 ? "blocked" : "pending";
    return task(`t_large_${String(i).padStart(4, "0")}`, [secIds[i % secIds.length]], {
      status,
      created_at: 10_000 + i,
      body: i % 19 === 0 ? `Regression body ${i}: ${"x".repeat(80 + (i % 40))}` : "",
    });
  });
  return plan(`pl_large_${taskCount}`, "orch_large", 900_000 + taskCount, sections, tasks, {
    primary_spec_id: `spec_large_${taskCount}`,
  });
}

export function largeCanvasLayout(taskCount: LargeCanvasWorldSize): WorkspaceLayout {
  return layoutWorkspace([largeCanvasPlan(taskCount)]);
}

export function largeCanvasFingerprint(taskCount: LargeCanvasWorldSize): string {
  return layoutFingerprint(largeCanvasLayout(taskCount));
}

export interface LargeCanvasLayoutStats {
  taskCount: number;
  coreNodeCount: number;
  bounds: WorkspaceLayout["bounds"];
  coreOverlapCount: number;
}

export function largeCanvasLayoutStats(taskCount: LargeCanvasWorldSize): LargeCanvasLayoutStats {
  const layout = largeCanvasLayout(taskCount);
  const frame = layout.frames.find((f) => f.planId === `pl_large_${taskCount}`);
  const core = frame ? frame.nodes.filter((n) => n.kind === "spec" || n.kind === "plan" || n.kind === "task") : coreSolidNodes(layout);
  return {
    taskCount: core.filter((n) => n.kind === "task").length,
    coreNodeCount: core.length,
    bounds: layout.bounds,
    coreOverlapCount: overlappingPairs(core).length,
  };
}
