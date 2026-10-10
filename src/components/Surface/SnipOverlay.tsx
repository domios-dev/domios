import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { clampToDraw, type PageRect, type Rect } from "./streamModel";
import { SNIP_HINT, SNIP_MIN_SIDE, snipSizeLabel, type PopoverAnchor } from "./surfaceChromeModel";

export interface SnipOverlayProps {
  drawRect(): Rect;
  pageRect(start: Point, end: Point): PageRect | null;
  /** `frame` is the selection in stage px, so the area stays outlined while the question is asked. */
  onFinish(rect: PageRect, anchor: PopoverAnchor, frame: Rect): void;
  onCancel(): void;
}

interface Point {
  x: number;
  y: number;
}

interface Drag {
  pointerId: number;
  start: Point;
  end: Point;
}

/** Client-side only: it sits above the stream and never sends input, so the agent keeps the page. */
export function SnipOverlay({ drawRect, pageRect, onFinish, onCancel }: SnipOverlayProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<Drag | null>(null);

  useEffect(() => {
    ref.current?.focus({ preventScroll: true });
  }, []);

  const local = (event: PointerEvent<HTMLDivElement>): Point => {
    const box = event.currentTarget.getBoundingClientRect();
    return clampToDraw({ x: event.clientX - box.left, y: event.clientY - box.top }, drawRect());
  };

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture?.(event.pointerId);
    const point = local(event);
    setDrag({ pointerId: event.pointerId, start: point, end: point });
  };

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    if (!drag || event.pointerId !== drag.pointerId) return;
    const end = local(event);
    setDrag({ ...drag, end });
  };

  const onPointerUp = (event: PointerEvent<HTMLDivElement>) => {
    if (!drag || event.pointerId !== drag.pointerId) return;
    const end = local(event);
    setDrag(null);
    const rect = pageRect(drag.start, end);
    if (!rect || rect.width < SNIP_MIN_SIDE || rect.height < SNIP_MIN_SIDE) return;
    const box = event.currentTarget.getBoundingClientRect();
    const frame = {
      x: Math.min(drag.start.x, end.x),
      y: Math.min(drag.start.y, end.y),
      width: Math.abs(end.x - drag.start.x),
      height: Math.abs(end.y - drag.start.y),
    };
    onFinish(rect, { x: frame.x + frame.width, y: frame.y + frame.height, width: box.width, height: box.height }, frame);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    onCancel();
  };

  const selection = drag && {
    left: Math.min(drag.start.x, drag.end.x),
    top: Math.min(drag.start.y, drag.end.y),
    width: Math.abs(drag.end.x - drag.start.x),
    height: Math.abs(drag.end.y - drag.start.y),
  };
  const size = drag && pageRect(drag.start, drag.end);

  return (
    <div
      ref={ref}
      role="application"
      aria-roledescription="area selection"
      aria-label={SNIP_HINT}
      tabIndex={0}
      data-testid="surface-snip"
      data-dragging={selection ? "true" : undefined}
      className="ct-surface-snip"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={() => setDrag(null)}
      onKeyDown={onKeyDown}
    >
      {selection ? (
        <div className="ct-surface-snip__box" style={selection}>
          {size && <span className="ct-surface-snip__size" data-testid="surface-snip-size">{snipSizeLabel(size)}</span>}
        </div>
      ) : (
        <p className="ct-surface-snip__hint">{SNIP_HINT}</p>
      )}
    </div>
  );
}

/** The snipped area outlined on the live page while its question is open; it never takes input. */
export function SnipFrame({ frame }: { frame: Rect }) {
  return (
    <div
      aria-hidden="true"
      data-testid="surface-snip-frame"
      className="ct-surface-snip-frame"
      style={{ left: frame.x, top: frame.y, width: frame.width, height: frame.height }}
    />
  );
}
