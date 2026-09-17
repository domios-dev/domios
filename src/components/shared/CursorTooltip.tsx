import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

const OFFSET_X = 16;
const OFFSET_Y = 18;
const CHASE = 0.22;

interface CursorTooltipProps {
  content: ReactNode;
  active: boolean;
  children: ReactNode;
  className?: string;
}

/**
 * A tip that trails the pointer. Lives on a wrapper because the control it
 * describes is usually disabled, and a disabled control emits no pointer events.
 */
export function CursorTooltip({ content, active, children, className }: CursorTooltipProps) {
  const [open, setOpen] = useState(false);
  const tipRef = useRef<HTMLDivElement>(null);
  const target = useRef({ x: 0, y: 0 });
  const shown = useRef({ x: 0, y: 0 });
  const frame = useRef<number | null>(null);

  const clamp = useCallback((x: number, y: number) => {
    const width = tipRef.current?.offsetWidth ?? 0;
    const height = tipRef.current?.offsetHeight ?? 0;
    return {
      x: Math.min(x, window.innerWidth - width - 8),
      y: Math.min(y, window.innerHeight - height - 8),
    };
  }, []);

  useEffect(() => {
    if (!open) return;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const step = () => {
      const node = tipRef.current;
      if (node) {
        const to = clamp(target.current.x, target.current.y);
        shown.current = reduce
          ? to
          : {
              x: shown.current.x + (to.x - shown.current.x) * CHASE,
              y: shown.current.y + (to.y - shown.current.y) * CHASE,
            };
        node.style.transform = `translate3d(${Math.round(shown.current.x)}px, ${Math.round(shown.current.y)}px, 0)`;
      }
      frame.current = requestAnimationFrame(step);
    };
    frame.current = requestAnimationFrame(step);
    return () => {
      if (frame.current !== null) cancelAnimationFrame(frame.current);
    };
  }, [open, clamp]);

  useEffect(() => {
    if (!active) setOpen(false);
  }, [active]);

  const track = (event: { clientX: number; clientY: number }) => {
    target.current = { x: event.clientX + OFFSET_X, y: event.clientY + OFFSET_Y };
  };

  return (
    <>
      <div
        className={className}
        onPointerEnter={(event) => {
          if (event.pointerType !== "mouse" || !active) return;
          track(event);
          shown.current = { ...target.current };
          setOpen(true);
        }}
        onPointerMove={(event) => {
          if (event.pointerType === "mouse") track(event);
        }}
        onPointerLeave={() => setOpen(false)}
      >
        {children}
      </div>
      {open &&
        createPortal(
          <div
            ref={tipRef}
            role="tooltip"
            className="ct-cursor-tip pointer-events-none fixed left-0 top-0 z-[60] whitespace-nowrap rounded-lg px-2.5 py-1.5 text-[11.5px] leading-none"
          >
            {content}
          </div>,
          document.body,
        )}
    </>
  );
}
