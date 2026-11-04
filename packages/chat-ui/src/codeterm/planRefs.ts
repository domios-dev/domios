import { referenceMarkers } from "./referenceMarkers";

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

export function linkifyPlanRefs(text: string): PlanRefSegment[] {
  const segments: PlanRefSegment[] = [];
  let cursor = 0;
  for (const marker of referenceMarkers(text)) {
    if (marker.kind === "tab" || (marker.kind !== "plan" && !marker.plan)) continue;
    if (marker.start > cursor) segments.push({ kind: "text", value: text.slice(cursor, marker.start) });
    segments.push({ kind: "plan_ref", value: marker.text, planId: marker.kind === "plan" ? marker.id : marker.plan!,
      targetId: marker.kind === "plan" ? undefined : marker.id, targetKind: marker.kind });
    cursor = marker.end;
  }
  if (cursor < text.length || !segments.length) segments.push({ kind: "text", value: text.slice(cursor) });
  return segments;
}

export function extractPlanRefs(text: string): PlanRefLinkSegment[] {
  const seen = new Set<string>();
  return linkifyPlanRefs(text).filter((segment): segment is PlanRefLinkSegment => {
    if (segment.kind !== "plan_ref") return false;
    const key = `${segment.targetKind}:${segment.planId}:${segment.targetId ?? ""}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}
export function hasPlanRefs(text: string): boolean { return extractPlanRefs(text).length > 0; }
