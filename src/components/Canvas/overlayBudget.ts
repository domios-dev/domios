import { overlayBudgetEnabled } from "./overlayBudgetFlag";
import { substrateV2Enabled } from "./substrateFlag";

const RICH_NODE_KINDS = new Set(["plan", "task", "spec"]);

export function isRichOverlayNodeKind(kind: string): boolean {
  return RICH_NODE_KINDS.has(kind);
}

export function overlayBudgetActive(): boolean {
  return substrateV2Enabled() && overlayBudgetEnabled();
}

export function shouldUseLightweightWorldCard(nodeKind: string): boolean {
  return overlayBudgetActive() && isRichOverlayNodeKind(nodeKind);
}

export function shouldSuppressWorldRichContent(nodeKind: string): boolean {
  return shouldUseLightweightWorldCard(nodeKind);
}

export function overlayRichIds(overlays: ReadonlyArray<{ id: string }>): Set<string> {
  return new Set(overlays.map((overlay) => overlay.id));
}
