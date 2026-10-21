import { useRef, useState, type KeyboardEvent, type PointerEvent } from "react";

export interface UnderwaySize { width: number; height: number }
const KEY = "domios.underway.size";
export function boundedUnderwaySize(size: UnderwaySize): UnderwaySize {
  return { width: Math.max(280, Math.min(1600, size.width)), height: Math.max(160, Math.min(800, size.height)) };
}
function readSize(): UnderwaySize | null {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(KEY) ?? "null");
    if (value && typeof value === "object" && "width" in value && "height" in value
      && typeof value.width === "number" && typeof value.height === "number"
      && Number.isFinite(value.width) && Number.isFinite(value.height)) return boundedUnderwaySize({ width: value.width, height: value.height });
  } catch { /* Local preferences may be unavailable in private viewers. */ }
  return null;
}

export function useUnderwayResize() {
  const [size, setSize] = useState(readSize);
  const drag = useRef<{ id: number; x: number; y: number; size: UnderwaySize } | null>(null);
  const current = useRef(size);
  const resize = (next: UnderwaySize, persist = false) => {
    current.current = boundedUnderwaySize(next);
    setSize(current.current);
    if (persist) try { localStorage.setItem(KEY, JSON.stringify(current.current)); } catch { /* A viewer can still resize without storage. */ }
  };
  const finish = () => {
    if (drag.current && current.current) resize(current.current, true);
    drag.current = null;
  };
  return { size, handle: {
    onPointerDown: (event: PointerEvent<HTMLButtonElement>) => {
      if (event.button !== 0) return;
      const card = event.currentTarget.closest(".mc-underway-frame");
      if (!card) return;
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      const rect = card.getBoundingClientRect();
      drag.current = { id: event.pointerId, x: event.clientX, y: event.clientY, size: { width: rect.width, height: rect.height } };
    },
    onPointerMove: (event: PointerEvent<HTMLButtonElement>) => {
      const start = drag.current;
      if (start?.id === event.pointerId) resize({ width: start.size.width - (event.clientX - start.x), height: start.size.height + event.clientY - start.y });
    },
    onPointerUp: finish,
    onPointerCancel: finish,
    onLostPointerCapture: finish,
    onKeyDown: (event: KeyboardEvent<HTMLButtonElement>) => {
      const value = current.current ?? { width: 520, height: 292 };
      const steps: Record<string, UnderwaySize> = {
        ArrowLeft: { ...value, width: value.width + 32 }, ArrowRight: { ...value, width: value.width - 32 },
        ArrowUp: { ...value, height: value.height - 32 }, ArrowDown: { ...value, height: value.height + 32 },
      };
      if (steps[event.code]) { event.preventDefault(); resize(steps[event.code], true); }
    },
  } };
}
