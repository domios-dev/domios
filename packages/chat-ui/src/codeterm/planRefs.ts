// Conservative inline linkification of `task_<32hex>` mentions in chat text.
// A task id only becomes a link when a `pl_<32hex>` plan id is present in the
// same text, so arbitrary hex tokens never turn into false links.

export interface TextSegment {
  kind: "text";
  value: string;
}
export interface TaskSegment {
  kind: "task";
  value: string;
  taskId: string;
  planId: string;
}
export type PlanRefSegment = TextSegment | TaskSegment;

const TASK_G = /task_[0-9a-f]{32}/g;
const PLAN_RE = /pl_[0-9a-f]{32}/;

/** Split text into plain + linkable task-id segments. No plan context → all text. */
export function linkifyPlanRefs(text: string): PlanRefSegment[] {
  if (!text) return [{ kind: "text", value: text ?? "" }];
  const planMatch = text.match(PLAN_RE);
  if (!planMatch) return [{ kind: "text", value: text }];
  const planId = planMatch[0];

  const segments: PlanRefSegment[] = [];
  let last = 0;
  for (const m of text.matchAll(TASK_G)) {
    const start = m.index ?? 0;
    if (start > last) segments.push({ kind: "text", value: text.slice(last, start) });
    segments.push({ kind: "task", value: m[0], taskId: m[0], planId });
    last = start + m[0].length;
  }
  if (segments.length === 0) return [{ kind: "text", value: text }];
  if (last < text.length) segments.push({ kind: "text", value: text.slice(last) });
  return segments;
}

/** True when text has at least one linkable task ref. */
export function hasPlanRefs(text: string): boolean {
  return linkifyPlanRefs(text).some((s) => s.kind === "task");
}
