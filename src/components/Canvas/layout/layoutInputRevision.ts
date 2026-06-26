import type { Plan, Section } from "../types";

function flatSectionIds(sections: Section[]): string[] {
  const out: string[] = [];
  const walk = (list: Section[]) => {
    for (const s of list) {
      out.push(s.id);
      if (s.subsections.length) walk(s.subsections);
    }
  };
  walk(sections);
  return out;
}

function sectionTreeKey(sections: Section[]): string {
  return flatSectionIds(sections).join(",");
}

// Fingerprint of every plan field that affects layoutWorkspace output. Computed
// every render so bounds recompute even if a parent reuses the plans array ref
// and mutates plan bodies in place.
export function layoutInputRevision(
  plans: Plan[],
  specSectionsByPlan?: ReadonlyMap<string, Section[]>,
): string {
  const parts: string[] = [];
  for (const p of plans) {
    if (p.status !== "active") continue;
    const specSections = p.primary_spec_id ? specSectionsByPlan?.get(p.id) : undefined;
    const sections = specSections?.length ? specSections : p.sections;
    parts.push(p.id, String(p.updated_at), String(p.primary_spec_id ?? ""), sectionTreeKey(sections));
    parts.push(String(p.tasks.length));
    for (const t of p.tasks) {
      parts.push(
        t.id,
        t.status,
        String(t.parent_task_id ?? ""),
        t.section_ids.join(","),
        String(t.body.length),
        String(t.intent?.length ?? 0),
        String(t.model_rationale?.length ?? 0),
        String(t.milestones.length),
        String(t.created_at ?? 0),
      );
    }
    parts.push(String(p.elements.length));
    for (const el of p.elements) {
      parts.push(el.id, el.kind, String(el.canvas.x ?? ""), String(el.canvas.y ?? ""));
    }
    parts.push(String(p.lineage?.length ?? 0));
    for (const link of p.lineage ?? []) {
      parts.push(link.to, link.rel);
    }
    parts.push(String(p.links?.length ?? 0));
    for (const link of p.links ?? []) {
      parts.push(link);
    }
  }
  return parts.join("\0");
}

export function plansForLayout(
  plans: Plan[],
  specSectionsByPlan?: ReadonlyMap<string, Section[]>,
): Plan[] {
  if (!specSectionsByPlan?.size) return plans;
  return plans.map((p) => {
    if (!p.primary_spec_id) return p;
    const specSections = specSectionsByPlan.get(p.id);
    if (!specSections?.length) return p;
    return { ...p, sections: specSections };
  });
}
