import { memo, useCallback } from "react";
import type { NodeRendererProps } from "./NodeRenderer";
import { BasicNode } from "./BasicNode";
import { colorForKind } from "../nodeKind";

function ExpandedNodeImpl(props: NodeRendererProps) {
  const { node, selected, collapsed, scale, activePulse, onSelect, onDragStart, onToggleCollapse } = props;
  // LOD: at low zoom render as a dot — simpler shapes from distance, mipmap-style
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

  const tags = node.tags ?? [];
  const shownTags = tags.slice(0, 3);
  const extraTags = tags.length - shownTags.length;
  const importance = Math.max(0, Math.min(1, (node.importance ?? 0) / 10));

  return (
    <div
      className="absolute w-[200px] h-[90px] p-2 box-border cursor-grab select-none pointer-events-auto rounded-lg flex flex-col gap-1 text-[11px] bg-[var(--ct-surface)] text-[var(--ct-fg)]"
      style={{
        left: (node.graph_x ?? 0) - 100,
        top: (node.graph_y ?? 0) - 45,
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
          className="absolute -inset-1 rounded-xl pointer-events-none"
          style={{
            border: `2px solid ${activePulse.color}`,
            animation: "mem-pulse-card 3s ease-out forwards",
          }}
        />
      )}
      <div className="flex items-center gap-1.5">
        <div
          className="w-2.5 h-2.5 rounded-full shrink-0"
          style={{ background: colorForKind(node.kind) }}
        />
        <div className="font-semibold overflow-hidden text-ellipsis whitespace-nowrap flex-1">
          {node.title}
        </div>
        <div
          title={`importance ${node.importance ?? 0}`}
          className="w-6 h-[3px] bg-white/10 rounded-[2px] overflow-hidden shrink-0"
        >
          <div
            className="h-full"
            style={{
              width: `${importance * 100}%`,
              background: colorForKind(node.kind),
            }}
          />
        </div>
      </div>
      <div
        className="flex-1 overflow-hidden text-[10px] leading-[1.3] text-[var(--ct-muted-fg)]"
        style={{
          display: "-webkit-box",
          WebkitLineClamp: 2,
          WebkitBoxOrient: "vertical",
        }}
      >
        {node.summary ?? ""}
      </div>
      {tags.length > 0 && (
        <div className="flex gap-[3px] flex-nowrap overflow-hidden">
          {shownTags.map((t) => (
            <span
              key={t}
              className="px-[5px] py-px bg-[var(--ct-hover)] rounded-[3px] text-[9px] whitespace-nowrap"
            >
              {t}
            </span>
          ))}
          {extraTags > 0 && (
            <span className="text-[9px] text-[var(--ct-muted-fg)]">+{extraTags}</span>
          )}
        </div>
      )}
    </div>
  );
}

export const ExpandedNode = memo(
  ExpandedNodeImpl,
  (prev, next) =>
    prev.node === next.node &&
    prev.selected === next.selected &&
    prev.collapsed === next.collapsed &&
    prev.scale === next.scale &&
    prev.activePulse === next.activePulse,
);
