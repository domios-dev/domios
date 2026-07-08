export interface TextSegment {
  kind: "text";
  value: string;
}
export interface PlanRefLinkSegment {
  kind: "plan_ref";
  value: string;
  planId: string;
  targetId?: string;
  targetKind: "plan" | "task" | "section";
}
export type PlanRefSegment = TextSegment | PlanRefLinkSegment;

const REF_G = /\b(?:pl|task|sec)_[a-z0-9]{1,32}\b/g;
const PLAN_RE = /\bpl_[a-z0-9]{1,32}\b/;

function targetKind(value: string): PlanRefLinkSegment["targetKind"] {
  if (value.startsWith("task_")) return "task";
  if (value.startsWith("sec_")) return "section";
  return "plan";
}

/** Split text into plain + linkable plan/task/section reference segments. */
export function linkifyPlanRefs(text: string): PlanRefSegment[] {
  if (!text) return [{ kind: "text", value: text ?? "" }];
  const planMatch = text.match(PLAN_RE);

  const segments: PlanRefSegment[] = [];
  let last = 0;
  for (const m of text.matchAll(REF_G)) {
    const value = m[0];
    const kind = targetKind(value);
    const planId = kind === "plan" ? value : planMatch?.[0];
    if (!planId) continue;
    const start = m.index ?? 0;
    if (start > last) segments.push({ kind: "text", value: text.slice(last, start) });
    segments.push({
      kind: "plan_ref",
      value,
      planId,
      targetId: kind === "plan" ? undefined : value,
      targetKind: kind,
    });
    last = start + value.length;
  }
  if (segments.length === 0) return [{ kind: "text", value: text }];
  if (last < text.length) segments.push({ kind: "text", value: text.slice(last) });
  return segments;
}

export function extractPlanRefs(text: string): PlanRefLinkSegment[] {
  const seen = new Set<string>();
  const refs: PlanRefLinkSegment[] = [];
  for (const seg of linkifyPlanRefs(text)) {
    if (seg.kind !== "plan_ref") continue;
    const key = `${seg.targetKind}:${seg.planId}:${seg.targetId ?? ""}`;
    if (seen.has(key)) continue;
    seen.add(key);
    refs.push(seg);
  }
  return refs;
}

/** True when text has at least one linkable plan/task/section ref. */
export function hasPlanRefs(text: string): boolean {
  return linkifyPlanRefs(text).some((s) => s.kind === "plan_ref");
}
