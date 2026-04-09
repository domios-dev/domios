import { memo, useCallback } from "react";
import type { NodeRendererProps } from "./NodeRenderer";
import { BasicNode } from "./BasicNode";
import { colorForKind, parseFirstImage } from "../nodeKind";

function ImageNodeImpl(props: NodeRendererProps) {
  const { node, selected, collapsed, scale, activePulse, onSelect, onDragStart, onToggleCollapse } = props;
  if (!selected && scale < 0.7) {
    return <BasicNode {...props} />;
  }

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

  const handleDoubleClick = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      onToggleCollapse(node.id);
    },
    [node.id, onToggleCollapse],
  );

  if (collapsed) {
    return <BasicNode {...props} />;
  }

  const img = node.body ? parseFirstImage(node.body) : null;
  const isRemote = img && /^https?:\/\//i.test(img.url);
  const importance = Math.max(0, Math.min(1, (node.importance ?? 0) / 10));

  return (
    <div
      className="absolute w-[200px] h-[150px] rounded-lg overflow-hidden cursor-grab select-none pointer-events-auto flex flex-col text-[11px] bg-[var(--ct-surface)] text-[var(--ct-fg)]"
      style={{
        left: (node.graph_x ?? 0) - 100,
        top: (node.graph_y ?? 0) - 75,
        border: selected ? "2px solid #fff" : "1px solid var(--ct-border)",
        boxShadow: selected
          ? "0 0 0 3px rgba(255,255,255,0.2)"
          : activePulse
            ? `0 0 16px ${activePulse.color}, 0 2px 6px rgba(0,0,0,0.4)`
            : "0 2px 6px rgba(0,0,0,0.4)",
        outline: activePulse ? `1px solid ${activePulse.color}` : "none",
      }}
      onMouseDown={handleMouseDown}
      onClick={handleClick}
      onDoubleClick={handleDoubleClick}
    >
      {activePulse && (
        <div
          key={activePulse.startTime}
          className="absolute -inset-1 rounded-xl pointer-events-none z-[2]"
          style={{
            border: `2px solid ${activePulse.color}`,
            animation: "mem-pulse-card 3s ease-out forwards",
          }}
        />
      )}
      <div className="flex-1 bg-black flex items-center justify-center overflow-hidden">
        {img && isRemote ? (
          <img
            src={img.url}
            alt={img.alt}
            draggable={false}
            className="max-w-full max-h-full object-cover"
          />
        ) : (
          <div className="text-[10px] text-[var(--ct-muted-fg)] p-1.5 text-center">
            🖼 {img?.alt || (node.body ? "local image" : "image")}
          </div>
        )}
      </div>
      <div className="px-2 py-1.5 flex items-center gap-1.5">
        <div
          className="w-2 h-2 rounded-full shrink-0"
          style={{ background: colorForKind(node.kind) }}
        />
        <div className="font-semibold overflow-hidden text-ellipsis whitespace-nowrap flex-1 text-[10px]">
          {node.title}
        </div>
        <div className="w-5 h-[3px] bg-white/10 rounded-[2px] overflow-hidden shrink-0">
          <div
            className="h-full"
            style={{
              width: `${importance * 100}%`,
              background: colorForKind(node.kind),
            }}
          />
        </div>
      </div>
    </div>
  );
}

export const ImageNode = memo(
  ImageNodeImpl,
  (prev, next) =>
    prev.node === next.node &&
    prev.selected === next.selected &&
    prev.collapsed === next.collapsed &&
    prev.scale === next.scale &&
    prev.activePulse === next.activePulse,
);
