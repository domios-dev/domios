import type { Plan, Section, Task } from "./types";

export type SectionRowGlyph = "done" | "progress" | "uncovered";

export interface SectionRow {
  section: Section;
  depth: number;
  taskCount: number;
  glyph: SectionRowGlyph;
}

function flatten(sections: Section[], depth: number, out: Section[]): void {
  for (const s of sections) {
    out.push(s);
    if (s.subsections.length) flatten(s.subsections, depth + 1, out);
  }
}

// Ordered flat list of every section (subsections inline after their parent).
// The layout's anchor math and the PlanNode card MUST iterate this same order.
export function flatSections(plan: Plan): Section[] {
  const out: Section[] = [];
  flatten(plan.sections, 0, out);
  return out;
}

function depthOf(sections: Section[], id: string, depth: number): number {
  for (const s of sections) {
    if (s.id === id) return depth;
    if (s.subsections.length) {
      const d = depthOf(s.subsections, id, depth + 1);
      if (d >= 0) return d;
    }
  }
  return -1;
}

// Per-row stats: how many tasks cover the section and whether they're all done,
// in progress, or the section is uncovered (n=0) — drives the row glyph (§2.1).
export function sectionRows(plan: Plan): SectionRow[] {
  const flat = flatSections(plan);
  const byId = new Map<string, Task[]>();
  for (const s of flat) byId.set(s.id, []);
  for (const t of plan.tasks) {
    for (const sid of t.section_ids) {
      const arr = byId.get(sid);
      if (arr) arr.push(t);
    }
  }
  return flat.map((section) => {
    const tasks = byId.get(section.id) ?? [];
    const taskCount = tasks.length;
    let glyph: SectionRowGlyph;
    if (taskCount === 0) glyph = "uncovered";
    else if (tasks.every((t) => t.status === "done" || t.status === "cancelled")) glyph = "done";
    else glyph = "progress";
    return {
      section,
      depth: depthOf(plan.sections, section.id, 0),
      taskCount,
      glyph,
    };
  });
}
