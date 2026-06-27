import { ghostPlanNodeId } from "./layout/lineage";

// Maps a fly-to chip's (kind, raw id) onto the canvas NODE id the camera should
// land on. The chips carry domain ids (a spec entity, a linked plan); the camera
// flies to nodes — so a spec chip lands on its plan's `${planId}::spec` node and a
// lineage chip lands on the linked plan, or its archived ghost when only that is on
// the board. Returns null when nothing flyable resolves (caller no-ops).
export function resolveFlyToNodeId(
  kind: string,
  rawId: string,
  planCtx: string | null,
  hasNode: (id: string) => boolean,
): string | null {
  const id = rawId.trim();
  // "spec" = the plan's bound spec node; "spec-section" = a specific spec section a
  // plan section derives from. Both land on the `${plan}::spec` node (sections live
  // inside it); the caller scrolls/pulses the target section after arrival.
  if (kind === "spec" || kind === "spec-section") {
    if (planCtx) {
      const specNode = `${planCtx}::spec`;
      if (hasNode(specNode)) return specNode;
    }
    return id && hasNode(id) ? id : null;
  }
  if (kind === "follows" || kind === "supersedes") {
    if (!id) return null;
    if (hasNode(id)) return id;
    const ghost = ghostPlanNodeId(id);
    return hasNode(ghost) ? ghost : null;
  }
  return id && hasNode(id) ? id : null;
}
