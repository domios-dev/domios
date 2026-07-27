import type { Tab } from "../../../lib/types";
import { buildTaskPresenceFingerprint } from "../canvasView/presenceFingerprint";
import { flattenSections } from "../sectionRows";
import type { Plan } from "../types";

// Fingerprint of plan + tab fields that affect buildMissionControl / modelToSnapshot.
// Status churn and tab heartbeats that do not touch these fields must not rebuild the stage.
export function missionControlInputRevision(plan: Plan, tabs: Record<string, Tab>): string {
  const parts: string[] = [
    plan.id,
    String(plan.updated_at),
    plan.title.trim(),
    clean(plan.goal?.vector),
    clean(plan.goal?.vision),
    String(plan.tasks.length),
    String(plan.loops.length),
  ];

  for (const t of plan.tasks) {
    if (t.status === "cancelled") continue;
    parts.push(
      t.id,
      t.status,
      String(t.status_changed_at ?? 0),
      String(t.updated_at ?? 0),
      clean(t.assigned_tab_id),
      clean(t.lane?.tab_id),
      clean(t.lane?.branch),
      clean(t.taken_by_role),
      clean(t.review_stage),
      clean(t.qa_state),
      String(t.milestones.length),
      String(t.milestones[t.milestones.length - 1]?.ts ?? 0),
      String(t.competition?.enabled ?? false),
      String(t.competition?.attempts ?? 0),
      String(t.execution_attempts?.length ?? 0),
      clean(t.lane?.cleanup_status),
      t.section_ids.join(","),
    );
  }

  for (const loop of plan.loops) {
    parts.push(loop.id, loop.status, String(loop.count), loop.target);
  }

  for (const s of flattenSections(plan.sections)) {
    parts.push(s.id, s.title.trim());
  }

  return `${parts.join("\0")}\x1e${buildTaskPresenceFingerprint([plan], tabs)}`;
}

function clean(value?: string | null): string {
  return value?.trim() ?? "";
}
