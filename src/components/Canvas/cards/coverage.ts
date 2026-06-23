import type { Plan, Section } from "../types";

function collectSectionIds(sections: Section[]): string[] {
  const ids: string[] = [];
  for (const section of sections) {
    ids.push(section.id);
    ids.push(...collectSectionIds(section.subsections));
  }
  return ids;
}

export function planCoverageFraction(plan: Plan): number {
  const sectionIds = collectSectionIds(plan.sections);
  if (sectionIds.length === 0) return 0;
  const covered = new Set<string>();
  for (const task of plan.tasks) {
    for (const sectionId of task.section_ids) {
      if (sectionIds.includes(sectionId)) covered.add(sectionId);
    }
  }
  return covered.size / sectionIds.length;
}
