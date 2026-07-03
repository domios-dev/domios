import type { Milestone, Plan } from "../types";
import { RETROSPECTIVE_SUPERSEDE_NOTE } from "./planRetrospectiveConstants";

export function latestRetrospective(plan: Plan): Milestone | null {
  const strides = plan.strides ?? [];
  let best: Milestone | null = null;
  for (const stride of strides) {
    if (stride.kind !== "retrospective") continue;
    if (stride.text.trim() === RETROSPECTIVE_SUPERSEDE_NOTE) continue;
    if (!best || stride.ts > best.ts) best = stride;
  }
  return best;
}

export function skillTimelineForSlug(plan: Plan, slug: string): Milestone[] {
  const needle = `skill ${slug}`;
  return (plan.strides ?? [])
    .filter((stride) => stride.kind === "skill" && stride.text.startsWith(needle))
    .sort((a, b) => a.ts - b.ts);
}

export function showRetrospectiveBlock(plan: Plan): boolean {
  if (latestRetrospective(plan)) return true;
  return plan.status === "archived" || plan.mode === "complete";
}
