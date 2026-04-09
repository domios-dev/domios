import { memo, useCallback } from "react";
import type { NodeRendererProps } from "./NodeRenderer";
import { colorForKind } from "../nodeKind";

function BasicNodeImpl({ node, selected, scale, activePulse, onSelect, onDragStart }: NodeRendererProps) {
  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      onDragStart(node.id, e);
    },
    [node.id, onDragStart],
  );

  const handleClick = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      onSelect(node.id);
    },
    [node.id, onSelect],
  );

  const fontSize = Math.max(8, Math.min(16, 11 / Math.max(scale, 0.6)));
  const showLabel = scale >= 0.4;

  return (
    <div
      className="mem-node absolute cursor-grab select-none pointer-events-auto -translate-x-1/2 -translate-y-1/2"
      style={{
        left: node.graph_x ?? 0,
        top: node.graph_y ?? 0,
      }}
      onMouseDown={handleMouseDown}
      onClick={handleClick}
    >
      <div className="relative w-[14px] h-[14px] mx-auto">
        <div
          className="w-[14px] h-[14px] rounded-full box-border"
          style={{
            background: colorForKind(node.kind),
            border: selected ? "2px solid #fff" : "2px solid rgba(0,0,0,0.4)",
            boxShadow: selected
              ? "0 0 0 3px rgba(255,255,255,0.3)"
              : activePulse
                ? `0 0 12px ${activePulse.color}`
                : "none",
          }}
        />
        {activePulse && (
          <div
            key={activePulse.startTime}
            className="absolute -inset-1.5 rounded-full pointer-events-none"
            style={{
              border: `2px solid ${activePulse.color}`,
              animation: "mem-pulse 3s ease-out forwards",
            }}
          />
        )}
      </div>
      {showLabel && (
        <div
          className="mt-1 text-center max-w-[160px] overflow-hidden text-ellipsis whitespace-nowrap pointer-events-none text-[var(--ct-fg)]"
          style={{
            fontSize,
            textShadow: "0 1px 2px rgba(0,0,0,0.8)",
          }}
        >
          {node.title}
        </div>
      )}
    </div>
  );
}

export const BasicNode = memo(
  BasicNodeImpl,
  (prev, next) =>
    prev.node === next.node &&
    prev.selected === next.selected &&
    prev.collapsed === next.collapsed &&
    prev.scale === next.scale &&
    prev.activePulse === next.activePulse,
);
