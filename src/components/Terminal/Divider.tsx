import { useCallback, useRef } from "react";
import type { Orientation } from "../../lib/types";

interface DividerProps {
  orientation: Orientation;
  onDrag: (delta: number) => void;
  onDragStart?: () => void;
  onDragEnd?: () => void;
  onDoubleClick: () => void;
}

export function Divider({ orientation, onDrag, onDragStart, onDragEnd, onDoubleClick }: DividerProps) {
  const dragging = useRef(false);
  const lastPos = useRef(0);

  const isHorizontal = orientation === "Horizontal";

  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      dragging.current = true;
      lastPos.current = isHorizontal ? e.clientX : e.clientY;
      onDragStart?.();

      const handleMouseMove = (ev: MouseEvent) => {
        if (!dragging.current) return;
        const pos = isHorizontal ? ev.clientX : ev.clientY;
        const delta = pos - lastPos.current;
        if (delta !== 0) {
          onDrag(delta);
          lastPos.current = pos;
        }
      };

      const handleMouseUp = () => {
        dragging.current = false;
        document.removeEventListener("mousemove", handleMouseMove);
        document.removeEventListener("mouseup", handleMouseUp);
        document.body.style.cursor = "";
        document.body.style.userSelect = "";
        onDragEnd?.();
      };

      document.addEventListener("mousemove", handleMouseMove);
      document.addEventListener("mouseup", handleMouseUp);
      document.body.style.cursor = isHorizontal ? "col-resize" : "row-resize";
      document.body.style.userSelect = "none";
    },
    [isHorizontal, onDrag, onDragStart, onDragEnd],
  );

  return (
    <div
      onMouseDown={handleMouseDown}
      onDoubleClick={onDoubleClick}
      style={{
        flexShrink: 0,
        width: isHorizontal ? "4px" : "100%",
        height: isHorizontal ? "100%" : "4px",
        cursor: isHorizontal ? "col-resize" : "row-resize",
        background: "#30363d",
        zIndex: 10,
      }}
      onMouseEnter={(e) => {
        (e.currentTarget as HTMLElement).style.background = "#58a6ff";
      }}
      onMouseLeave={(e) => {
        (e.currentTarget as HTMLElement).style.background = "#30363d";
      }}
    />
  );
}
