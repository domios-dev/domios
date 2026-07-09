import type { PresenceAgent } from "./presence/identity";
import type { Task } from "./types";

export interface SectionAgentSource {
  agents: PresenceAgent[];
  live: boolean;
}

export function rollUpSectionAgents(
  tasks: readonly Task[],
  sourceById: ReadonlyMap<string, SectionAgentSource>,
): Map<string, PresenceAgent[]> {
  const bySection = new Map<string, PresenceAgent[]>();
  for (const t of tasks) {
    const src = sourceById.get(t.id);
    if (!src?.live || !src.agents.length) continue;
    for (const sid of t.section_ids) {
      let arr = bySection.get(sid);
      if (!arr) {
        arr = [];
        bySection.set(sid, arr);
      }
      for (const a of src.agents) if (!arr.some((x) => x.id === a.id)) arr.push(a);
    }
  }
  return bySection;
}
