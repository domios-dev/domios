import type { Tab } from "../../../lib/types";
import type { Plan } from "../types";

export function buildTaskPresenceFingerprint(filteredPlans: readonly Plan[], tabs: Record<string, Tab>): string {
  const ids = new Set<string>();
  const add = (v?: string | null) => {
    const s = v?.trim();
    if (s) ids.add(s);
  };
  const assignmentParts: string[] = [];
  for (const p of filteredPlans) {
    const hb = p.heartbeat;
    assignmentParts.push(
      p.id,
      hb?.current_focus ?? "",
      (hb?.active_lanes ?? []).join(","),
    );
    for (const t of p.tasks) {
      add(t.assigned_tab_id);
      add(t.lane?.tab_id);
      for (const m of t.milestones) add(m.from?.tab_id);
      assignmentParts.push(t.id, t.assigned_tab_id ?? "", t.lane?.tab_id ?? "");
    }
  }
  const parts: string[] = [];
  for (const id of [...ids].sort()) {
    const tab = tabs[id];
    parts.push(id, tab ? tab.title ?? "" : "\x00");
  }
  return `${parts.join("\x1f")}\x1d${assignmentParts.join("\x1e")}`;
}
