import { useCallback, useRef, useState } from "react";
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
  const [isActive, setIsActive] = useState(false);
  const [isHovered, setIsHovered] = useState(false);

  const isHorizontal = orientation === "Horizontal";

  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      dragging.current = true;
      lastPos.current = isHorizontal ? e.clientX : e.clientY;
      setIsActive(true);
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
        setIsActive(false);
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

  const showHandle = isHovered || isActive;
  const accentColor = "var(--ct-accent, #6366f1)";
  const subtleColor = "var(--ct-border-subtle, rgba(255, 255, 255, 0.06))";
  const mutedColor = "var(--ct-muted, #484f58)";

  return (
    <div
      onMouseDown={handleMouseDown}
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
      onDoubleClick={onDoubleClick}
      style={{
        position: "relative",
        flexShrink: 0,
        width: isHorizontal ? "1px" : "100%",
        height: isHorizontal ? "100%" : "1px",
        cursor: isHorizontal ? "col-resize" : "row-resize",
        zIndex: 10,
      }}
    >
      {/* Invisible 12px hit area for easy grabbing */}
      <div
        style={{
          position: "absolute",
          ...(isHorizontal
            ? { left: "-6px", top: 0, bottom: 0, width: "12px" }
            : { top: "-6px", left: 0, right: 0, height: "12px" }),
          zIndex: 1,
        }}
      />

      {/* Visible 1px line */}
      <div
        style={{
          position: "absolute",
          backgroundColor: isActive || isHovered ? accentColor : subtleColor,
          transition: "background-color 120ms cubic-bezier(0.16, 1, 0.3, 1)",
          ...(isHorizontal
            ? { left: 0, top: 0, bottom: 0, width: "1px" }
            : { top: 0, left: 0, right: 0, height: "1px" }),
        }}
      />

      {/* Grab handle — centered pill shape, appears on hover/active */}
      <div
        style={{
          position: "absolute",
          borderRadius: "3px",
          backgroundColor: isActive ? accentColor : mutedColor,
          zIndex: 2,
          opacity: showHandle ? 1 : 0,
          transition: isHorizontal
            ? "opacity 120ms cubic-bezier(0.16, 1, 0.3, 1), height 120ms cubic-bezier(0.16, 1, 0.3, 1), background-color 120ms cubic-bezier(0.16, 1, 0.3, 1)"
            : "opacity 120ms cubic-bezier(0.16, 1, 0.3, 1), width 120ms cubic-bezier(0.16, 1, 0.3, 1), background-color 120ms cubic-bezier(0.16, 1, 0.3, 1)",
          ...(isHorizontal
            ? {
                width: "3px",
                height: isActive ? "64px" : "32px",
                left: "-1px",
                top: "50%",
                transform: "translateY(-50%)",
              }
            : {
                height: "3px",
                width: isActive ? "64px" : "32px",
                top: "-1px",
                left: "50%",
                transform: "translateX(-50%)",
              }),
        }}
      />
    </div>
  );
}
