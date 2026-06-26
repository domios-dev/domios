import { overlayBudgetEnabled } from "./overlayBudgetFlag";

const RICH_NODE_KINDS = new Set(["plan", "task", "spec"]);

export function isRichOverlayNodeKind(kind: string): boolean {
  return RICH_NODE_KINDS.has(kind);
}

export function shouldSuppressWorldRichContent(nodeKind: string): boolean {
  return overlayBudgetEnabled() && isRichOverlayNodeKind(nodeKind);
}

export function overlayRichIds(overlays: ReadonlyArray<{ id: string }>): Set<string> {
  return new Set(overlays.map((overlay) => overlay.id));
}
