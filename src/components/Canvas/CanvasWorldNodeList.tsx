import { CanvasNodeCard } from "./CanvasNodeCard";
import type { CanvasLod } from "./cards";
import type { LaidOutNode } from "./layout";
import { shouldUseLightweightWorldCard } from "./overlayBudget";
import type { World } from "./worlds";

export interface CanvasWorldNodeListProps {
  nodes: LaidOutNode[];
  lod: CanvasLod;
  worldByNode: Map<string, World>;
  crispRasterScale: (world: World, scale: number) => number;
  transformScale: number;
  dominantNodeId: string | null;
  resolveModel: (node: LaidOutNode) => unknown;
  onOpenPane?: (paneId: string) => void;
  onHoverTask?: (taskId: string | null) => void;
  onArchive?: (planId: string) => void;
  onFitNode?: (nodeId: string) => void;
  onCopyLink?: (id: string) => void;
  onFocusStride?: (milestoneId: string) => void;
  onSectionClick?: (a: import("./nodeRegistry").SectionNavArgs) => void;
  onSectionFrame?: (a: import("./nodeRegistry").SectionNavArgs) => void;
  tagFilterActive: boolean;
  tagFilterMatchIds: Set<string> | null;
}

export function CanvasWorldNodeList({
  nodes,
  lod,
  worldByNode,
  crispRasterScale,
  transformScale,
  dominantNodeId,
  resolveModel,
  onOpenPane,
  onHoverTask,
  onArchive,
  onFitNode,
  onCopyLink,
  onFocusStride,
  onSectionClick,
  onSectionFrame,
  tagFilterActive,
  tagFilterMatchIds,
}: CanvasWorldNodeListProps) {
  return (
    <>
      {nodes.map((n) => {
        const nodeWorld = worldByNode.get(n.id) ?? "W1";
        const tagFilterDimmed = tagFilterActive && n.kind === "task" && !tagFilterMatchIds!.has(n.id);
        const tagFilterHighlighted = tagFilterActive && n.kind === "task" && tagFilterMatchIds!.has(n.id);
        return (
          <CanvasNodeCard
            key={n.id}
            node={n}
            model={resolveModel(n)}
            lod={lod}
            world={nodeWorld}
            crispScale={crispRasterScale(nodeWorld, transformScale)}
            elevated={dominantNodeId != null && n.id === dominantNodeId}
            onOpenPane={onOpenPane}
            onHoverTask={onHoverTask}
            onArchive={n.kind === "plan" && onArchive ? () => onArchive(n.id) : undefined}
            onFitNode={n.kind === "plan" ? onFitNode : undefined}
            onCopyLink={onCopyLink}
            onFocusStride={onFocusStride}
            onSectionClick={onSectionClick}
            onSectionFrame={onSectionFrame}
            tagFilterDimmed={tagFilterDimmed}
            tagFilterHighlighted={tagFilterHighlighted}
            suppressRichContent={shouldUseLightweightWorldCard(n.kind)}
            lightweightWorld={shouldUseLightweightWorldCard(n.kind)}
          />
        );
      })}
    </>
  );
}
