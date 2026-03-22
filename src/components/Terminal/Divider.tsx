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

  // Thin visible line (1px) inside a wider hit target (6px)
  return (
    <div
      onMouseDown={handleMouseDown}
      onDoubleClick={onDoubleClick}
      className="group/divider relative shrink-0"
      style={{
        width: isHorizontal ? "6px" : "100%",
        height: isHorizontal ? "100%" : "6px",
        cursor: isHorizontal ? "col-resize" : "row-resize",
        zIndex: 10,
      }}
    >
      {/* Visible line */}
      <div
        className="absolute transition-colors duration-150 group-hover/divider:bg-[var(--ct-accent)]"
        style={{
          backgroundColor: "var(--ct-border)",
          ...(isHorizontal
            ? { left: "2px", top: 0, bottom: 0, width: "1px" }
            : { top: "2px", left: 0, right: 0, height: "1px" }),
        }}
      />
    </div>
  );
}
