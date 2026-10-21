import { useRef, useState, type KeyboardEvent, type PointerEvent } from "react";

export interface UnderwaySize { width: number; height: number }
export const UNDERWAY_MIN = { width: 320, height: 200 };
const MAX = { width: 1600, height: 800 };
const ROW_GAP = 12;

export function boundedUnderwaySize(size: UnderwaySize, maxWidth = MAX.width): UnderwaySize {
  const width = Math.max(UNDERWAY_MIN.width, Math.min(maxWidth, MAX.width, size.width));
  return { width, height: Math.max(UNDERWAY_MIN.height, Math.min(MAX.height, size.height)) };
}

/** Both corner grips resize the shared row: Underway's width, and with it what the neighbour keeps. */
function measure(handle: HTMLElement) {
  const row = handle.closest(".mc-board-live-row");
  const card = row?.querySelector(".mc-underway-frame");
  if (!row || !card) return null;
  const rect = card.getBoundingClientRect();
  const neighbour = row.querySelector(".mc-reflexes-frame") ? UNDERWAY_MIN.width + ROW_GAP : 0;
  return { size: { width: rect.width, height: rect.height }, maxWidth: row.getBoundingClientRect().width - neighbour };
}

export function useUnderwayResize() {
  const [size, setSize] = useState<UnderwaySize | null>(null);
  const drag = useRef<{ id: number; x: number; y: number; size: UnderwaySize; maxWidth: number } | null>(null);
  const resize = (next: UnderwaySize, maxWidth: number) => setSize(boundedUnderwaySize(next, maxWidth));
  const finish = () => { drag.current = null; };
  return { size, handle: {
    onPointerDown: (event: PointerEvent<HTMLButtonElement>) => {
      if (event.button !== 0) return;
      const start = measure(event.currentTarget);
      if (!start) return;
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      drag.current = { id: event.pointerId, x: event.clientX, y: event.clientY, ...start };
    },
    onPointerMove: (event: PointerEvent<HTMLButtonElement>) => {
      const start = drag.current;
      if (start?.id === event.pointerId) resize({ width: start.size.width - (event.clientX - start.x), height: start.size.height + event.clientY - start.y }, start.maxWidth);
    },
    onPointerUp: finish,
    onPointerCancel: finish,
    onLostPointerCapture: finish,
    onKeyDown: (event: KeyboardEvent<HTMLButtonElement>) => {
      const start = measure(event.currentTarget);
      if (!start) return;
      const value = start.size;
      const steps: Record<string, UnderwaySize> = {
        ArrowLeft: { ...value, width: value.width + 32 }, ArrowRight: { ...value, width: value.width - 32 },
        ArrowUp: { ...value, height: value.height - 32 }, ArrowDown: { ...value, height: value.height + 32 },
      };
      if (steps[event.code]) { event.preventDefault(); resize(steps[event.code], start.maxWidth); }
    },
  } };
}
