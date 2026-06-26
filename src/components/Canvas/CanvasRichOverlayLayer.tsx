import { useMemo } from "react";
import type { CanvasLod } from "./cards";
import { CanvasNodeCard } from "./CanvasNodeCard";
import type { LaidOutNode } from "./layout";
import type { OverlayNode } from "./richOverlays";
import { substrateV2Enabled } from "./substrateFlag";
import type { World } from "./worlds";

const OVERLAY_LOD: CanvasLod = "near";
const OVERLAY_WORLD: World = "W3";

export interface CanvasRichOverlayLayerProps {
  overlays: readonly OverlayNode[];
  nodeById: Map<string, LaidOutNode>;
  resolveModel: (node: LaidOutNode) => unknown;
  onOpenPane?: (paneId: string) => void;
  onHoverTask?: (taskId: string | null) => void;
  onArchive?: (planId: string) => void;
  onFitNode?: (nodeId: string) => void;
  onCopyLink?: (id: string) => void;
  onFocusStride?: (milestoneId: string) => void;
  onSectionClick?: (a: import("./nodeRegistry").SectionNavArgs) => void;
  onSectionFrame?: (a: import("./nodeRegistry").SectionNavArgs) => void;
}

function overlayStyle(rect: OverlayNode["screenRect"]) {
  return {
    position: "absolute" as const,
    left: rect.x,
    top: rect.y,
    width: rect.width,
    height: rect.height,
    zIndex: 30,
    pointerEvents: "auto" as const,
  };
}

export function CanvasRichOverlayLayer({
  overlays,
  nodeById,
  resolveModel,
  onOpenPane,
  onHoverTask,
  onArchive,
  onFitNode,
  onCopyLink,
  onFocusStride,
  onSectionClick,
  onSectionFrame,
}: CanvasRichOverlayLayerProps) {
  const enabled = substrateV2Enabled();
  const items = useMemo(
    () =>
      overlays
        .map((overlay) => {
          const node = nodeById.get(overlay.id);
          if (!node) return null;
          return { overlay, node, model: resolveModel(node) };
        })
        .filter((item): item is NonNullable<typeof item> => item != null && item.model != null),
    [overlays, nodeById, resolveModel],
  );

  if (!enabled || items.length === 0) return null;

  return (
    <div data-testid="canvas-rich-overlay-layer" className="pointer-events-none absolute inset-0" style={{ zIndex: 30 }}>
      {items.map(({ overlay, node, model }) => (
        <div
          key={overlay.id}
          data-testid="canvas-rich-overlay"
          data-overlay-id={overlay.id}
          data-overlay-kind={overlay.kind}
          data-overlay-reason={overlay.reason}
          style={overlayStyle(overlay.screenRect)}
        >
          <CanvasNodeCard
            node={node}
            model={model}
            lod={OVERLAY_LOD}
            world={OVERLAY_WORLD}
            crispScale={1}
            onOpenPane={onOpenPane}
            onHoverTask={onHoverTask}
            onArchive={node.kind === "plan" && onArchive ? () => onArchive(node.id) : undefined}
            onFitNode={node.kind === "plan" ? onFitNode : undefined}
            onCopyLink={onCopyLink}
            onFocusStride={onFocusStride}
            onSectionClick={onSectionClick}
            onSectionFrame={onSectionFrame}
            richOverlay
            fillParent
          />
        </div>
      ))}
    </div>
  );
}
