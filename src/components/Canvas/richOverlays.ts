import type { Box } from "./layout";
import { projectWorldBox, type CanvasScene, type SceneNodeRecord } from "./scene";
import type { ViewportTransform } from "./useCanvasViewport";

export const MAX_ACTIVE_RICH_OVERLAYS = 2;

export type OverlayReason = "focused" | "selected";

export interface OverlayCapabilities {
  markdown: true;
  mermaid: true;
  copy: true;
}

export interface OverlayNode {
  id: string;
  kind: "plan" | "task" | "spec";
  reason: OverlayReason;
  worldBox: Box;
  screenRect: Box;
  capabilities: OverlayCapabilities;
  preservesWorldBox: true;
}

export interface BuildOverlayNodesInput {
  scene: CanvasScene;
  transform: ViewportTransform;
  focusedId?: string | null;
  selectedIds?: readonly string[];
  maxActive?: number;
}

function cloneBox(box: Box): Box {
  return { x: box.x, y: box.y, width: box.width, height: box.height };
}

function overlayKind(node: SceneNodeRecord): OverlayNode["kind"] | null {
  return node.kind === "plan" || node.kind === "task" || node.kind === "spec" ? node.kind : null;
}

export function buildOverlayNodes({
  scene,
  transform,
  focusedId = null,
  selectedIds = [],
  maxActive = MAX_ACTIVE_RICH_OVERLAYS,
}: BuildOverlayNodesInput): OverlayNode[] {
  const byId = new Map(scene.nodes.map((node) => [node.id, node]));
  const requested: Array<[string, OverlayReason]> = [];
  if (focusedId) requested.push([focusedId, "focused"]);
  for (const id of selectedIds) requested.push([id, "selected"]);

  const seen = new Set<string>();
  const overlays: OverlayNode[] = [];
  for (const [id, reason] of requested) {
    if (seen.has(id) || overlays.length >= maxActive) continue;
    const node = byId.get(id);
    const kind = node ? overlayKind(node) : null;
    if (!node || !kind) continue;
    seen.add(id);
    overlays.push({
      id,
      kind,
      reason,
      worldBox: cloneBox(node.worldBox),
      screenRect: projectWorldBox(node.worldBox, transform),
      capabilities: { markdown: true, mermaid: true, copy: true },
      preservesWorldBox: true,
    });
  }
  return overlays;
}
