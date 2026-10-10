import { useRef, useState } from "react";
import { dragLogHeight, keyLogHeight, logHeightBounds } from "./dividerModel";

/** The resizable boundary between graph and run log; dragging or arrow keys set the log height. */
export function RunDivider({ height, available, onResize, controls }: { height: number; available: number; onResize: (height: number) => void; controls: string }) {
  const drag = useRef<{ id: number; y: number; height: number } | null>(null);
  const [dragging, setDragging] = useState(false);
  const { min, max } = logHeightBounds(available);
  return <div
    role="separator"
    aria-orientation="horizontal"
    aria-label="Resize run log"
    aria-controls={controls}
    aria-valuemin={min}
    aria-valuemax={max}
    aria-valuenow={height}
    tabIndex={0}
    className="bp-divider"
    data-dragging={dragging || undefined}
    onPointerDown={(event) => {
      if (event.button !== 0) return;
      event.preventDefault();
      event.currentTarget.setPointerCapture?.(event.pointerId);
      drag.current = { id: event.pointerId, y: event.clientY, height };
      setDragging(true);
    }}
    onPointerMove={(event) => {
      const start = drag.current;
      if (start?.id === event.pointerId) onResize(dragLogHeight(start.height, start.y, event.clientY, available));
    }}
    onPointerUp={() => { drag.current = null; setDragging(false); }}
    onPointerCancel={() => { drag.current = null; setDragging(false); }}
    onKeyDown={(event) => {
      const next = keyLogHeight(height, event.code, available);
      if (next === null) return;
      event.preventDefault();
      onResize(next);
    }}
  />;
}
