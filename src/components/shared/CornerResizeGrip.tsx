import { useRef, useState, type ButtonHTMLAttributes, type KeyboardEvent, type PointerEvent } from "react";

/** The card's own corner as a resize grip (see cornerGrip.css); callers supply the drag and key handlers. */
export function CornerResizeGrip({ side = "right", vertical = false, className = "", ...props }: ButtonHTMLAttributes<HTMLButtonElement> & {
  side?: "left" | "right"; vertical?: boolean;
}) {
  const classes = ["ct-corner-grip", side === "left" && "ct-corner-grip--left", vertical && "ct-corner-grip--vertical", className].filter(Boolean).join(" ");
  return <button type="button" title="Drag to resize" {...props} className={classes} />;
}

const STEP = 32;

/** Height-only resizing for one element: drag the grip or press ArrowUp/ArrowDown on it; the height settles within min and max. */
export function useVerticalResize<T extends HTMLElement>({ min, max, initial = null, onResized }: {
  min: number; max: number; initial?: number | null; onResized?: (height: number) => void;
}) {
  const target = useRef<T>(null);
  const [height, setHeight] = useState<number | null>(initial === null ? null : Math.max(min, Math.min(max, initial)));
  const latest = useRef(height);
  const drag = useRef<{ id: number; y: number; height: number } | null>(null);
  const current = () => latest.current ?? (target.current?.getBoundingClientRect().height || min);
  const resize = (next: number) => {
    const value = Math.round(Math.max(min, Math.min(max, next)));
    latest.current = value;
    setHeight(value);
  };
  const finish = () => {
    if (!drag.current) return;
    drag.current = null;
    if (latest.current !== null) onResized?.(latest.current);
  };
  return { height, target, grip: {
    onPointerDown: (event: PointerEvent<HTMLButtonElement>) => {
      if (event.button !== 0) return;
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      drag.current = { id: event.pointerId, y: event.clientY, height: current() };
    },
    onPointerMove: (event: PointerEvent<HTMLButtonElement>) => {
      const start = drag.current;
      if (start?.id === event.pointerId) resize(start.height + event.clientY - start.y);
    },
    onPointerUp: finish,
    onPointerCancel: finish,
    onLostPointerCapture: finish,
    onKeyDown: (event: KeyboardEvent<HTMLButtonElement>) => {
      if (event.code !== "ArrowUp" && event.code !== "ArrowDown") return;
      event.preventDefault();
      resize(current() + (event.code === "ArrowDown" ? STEP : -STEP));
      onResized?.(latest.current!);
    },
  } };
}
