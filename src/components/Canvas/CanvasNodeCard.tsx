import { Fragment, memo } from "react";
import type { CanvasLod } from "./cards";
import { getNodeRenderer, type SectionNavArgs } from "./nodeRegistry";
import type { LaidOutNode } from "./layout";
import type { World } from "./worlds";

interface CanvasNodeCardProps {
  node: LaidOutNode;
  model: unknown;
  lod: CanvasLod;
  world: World;
  crispScale: number;
  elevated?: boolean;
  onOpenPane?: (paneId: string) => void;
  onHoverTask?: (taskId: string | null) => void;
  onArchive?: () => void;
  onFitNode?: (nodeId: string) => void;
  onCopyLink?: (id: string) => void;
  onFocusStride?: (milestoneId: string) => void;
  onSectionClick?: (a: SectionNavArgs) => void;
  onSectionFrame?: (a: SectionNavArgs) => void;
  tagFilterDimmed?: boolean;
  tagFilterHighlighted?: boolean;
  suppressRichContent?: boolean;
  richOverlay?: boolean;
  lightweightWorld?: boolean;
  fillParent?: boolean;
}

function nodeStyle(box: { x: number; y: number; width: number; height?: number }) {
  return {
    position: "absolute" as const,
    left: box.x,
    top: box.y,
    width: box.width,
    height: box.height,
  };
}

export const CanvasNodeCard = memo(
  function CanvasNodeCard({ node, model, lod, world, crispScale, elevated, onOpenPane, onHoverTask, onArchive, onFitNode, onCopyLink, onFocusStride, onSectionClick, onSectionFrame, tagFilterDimmed, tagFilterHighlighted, suppressRichContent, richOverlay, lightweightWorld, fillParent }: CanvasNodeCardProps) {
    const base = fillParent
      ? { position: "absolute" as const, left: 0, top: 0, width: "100%", height: "100%" }
      : nodeStyle(node.box);
    const style: typeof base & { zIndex?: number; transition?: string; filter?: string } =
      elevated
        ? { ...base, zIndex: 20, filter: "drop-shadow(0 18px 40px rgba(0,0,0,0.55))", transition: "filter var(--ct-duration-normal) var(--ct-ease-decel)" }
        : base;
    const rendered = getNodeRenderer(node.kind)({
      node,
      model,
      style,
      lod,
      world,
      crispScale,
      onOpenPane,
      onHoverTask,
      onArchive,
      onFitNode,
      onCopyLink,
      onCopyStrideLink: onCopyLink,
      onFocusStride,
      onSectionClick,
      onSectionFrame,
      tagFilterDimmed,
      tagFilterHighlighted,
      suppressRichContent,
      richOverlay,
      lightweightWorld,
    });
    return rendered ? <Fragment key={node.id}>{rendered}</Fragment> : null;
  },
  (prev, next) => {
    if (prev.node.id !== next.node.id) return false;
    if (prev.lod !== next.lod) return false;
    if (prev.world !== next.world) return false;
    if (prev.crispScale !== next.crispScale) return false;
    if (prev.elevated !== next.elevated) return false;
    if (prev.tagFilterDimmed !== next.tagFilterDimmed) return false;
    if (prev.tagFilterHighlighted !== next.tagFilterHighlighted) return false;
    if (prev.suppressRichContent !== next.suppressRichContent) return false;
    if (prev.richOverlay !== next.richOverlay) return false;
    if (prev.lightweightWorld !== next.lightweightWorld) return false;
    if (prev.fillParent !== next.fillParent) return false;
    if (prev.model !== next.model) return false;
    const a = prev.node.box;
    const b = next.node.box;
    return a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height;
  },
);
