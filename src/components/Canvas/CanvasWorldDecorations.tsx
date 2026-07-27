import { memo } from "react";
import type { LaidOutNode } from "./layout";

interface CanvasWorldDecorationsProps {
  nodeById: Map<string, LaidOutNode>;
  nodeIds: Set<string>;
  hoveredTask: string | null;
  highlight: { sectionId: string; taskIds: Set<string> } | null;
  tagFilterMatchIds: Set<string> | null;
  blinkIds: ReadonlySet<string>;
  pulsePlanIds: ReadonlySet<string>;
}

function boxStyle(box: { x: number; y: number; width: number; height: number }, radius: string) {
  return { left: box.x, top: box.y, width: box.width, height: box.height, borderRadius: radius };
}

export const CanvasWorldDecorations = memo(function CanvasWorldDecorations({
  nodeById,
  nodeIds,
  hoveredTask,
  highlight,
  tagFilterMatchIds,
  blinkIds,
  pulsePlanIds,
}: CanvasWorldDecorationsProps) {
  return (
    <>
      {hoveredTask && nodeById.has(hoveredTask) && nodeIds.has(hoveredTask) && (
        <div className="ct-canvas-hovered pointer-events-none absolute" style={boxStyle(nodeById.get(hoveredTask)!.box, "var(--ct-radius-md)")} />
      )}
      {highlight && [...highlight.taskIds].map((id) => {
        const box = nodeById.get(id)?.box;
        if (!box || !nodeIds.has(id)) return null;
        return (
          <div key={`hl-${id}`} data-testid="canvas-section-highlight" className="ct-canvas-hovered pointer-events-none absolute" style={boxStyle(box, "var(--ct-radius-md)")} />
        );
      })}
      {tagFilterMatchIds && [...tagFilterMatchIds].map((id) => {
        const box = nodeById.get(id)?.box;
        if (!box || !nodeIds.has(id)) return null;
        return (
          <div key={`tag-hl-${id}`} data-testid="canvas-tag-filter-highlight" className="ct-canvas-hovered pointer-events-none absolute" style={boxStyle(box, "var(--ct-radius-md)")} />
        );
      })}
      {[...blinkIds].map((id) => {
        const box = nodeById.get(id)?.box;
        if (!box || !nodeIds.has(id)) return null;
        return (
          <div key={`blink-${id}`} data-testid="canvas-fly-blink" data-blink-id={id} className="ct-canvas-blink pointer-events-none absolute" style={boxStyle(box, "var(--ct-radius-lg)")} />
        );
      })}
      {[...pulsePlanIds].map((id) => {
        const box = nodeById.get(id)?.box;
        if (!box || !nodeIds.has(id)) return null;
        return (
          <div key={`pulse-${id}`} data-testid="canvas-change-pulse" className="ct-canvas-hovered pointer-events-none absolute" style={boxStyle(box, "var(--ct-radius-lg)")} />
        );
      })}
    </>
  );
});
