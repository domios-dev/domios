import { ghostPlanNodeId } from "./layout/lineage";

export function resolveFlyToNodeId(
  kind: string,
  rawId: string,
  planCtx: string | null,
  hasNode: (id: string) => boolean,
): string | null {
  const id = rawId.trim();
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
