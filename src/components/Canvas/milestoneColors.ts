import { MILESTONE_LANE_COLORS } from "./settings";
import type { Plan } from "./types";

// Resolve each milestone kind to its lane color: declared color wins, else a
// stable fallback by declared-lane index (§2.4 color lanes).
export function milestoneColorMap(plan: Plan): Map<string, string> {
  const map = new Map<string, string>();
  let next = 0;
  const assign = (kind: string, declared: string | null) => {
    if (map.has(kind)) return;
    const color = declared ?? MILESTONE_LANE_COLORS[next % MILESTONE_LANE_COLORS.length];
    map.set(kind, color);
    next++;
  };
  for (const k of plan.milestone_kinds) assign(k.key, k.color);
  for (const t of plan.tasks) for (const m of t.milestones) assign(m.kind, null);
  return map;
}
