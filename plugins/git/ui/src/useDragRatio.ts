import { useCallback, useEffect, useRef, useState } from "react";

export type DragAxis = "x" | "y";

interface Options {
  storageKey: string;
  axis: DragAxis;
  minPx?: number;
  defaultRatio?: number;
}

const DEFAULT_MIN_PX = 200;
const DEFAULT_RATIO = 0.5;
const HARD_LO = 0.15;
const HARD_HI = 0.85;

// The plugin iframe is sandboxed (allow-scripts, origin null) — touching
// localStorage throws a SecurityError. Guard every access so persistence
// silently degrades to in-memory instead of crashing the panel on mount.
function safeGet(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function safeSet(key: string, value: string): void {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    /* sandboxed iframe — no persistence */
  }
}

function loadRatio(key: string, fallback: number): number {
  if (typeof window === "undefined") return fallback;
  const raw = Number(safeGet(key));
  return raw > HARD_LO && raw < HARD_HI ? raw : fallback;
}

// Generic draggable divider ratio (leading-pane fraction) for either axis,
// persisted to localStorage, clamped so neither side drops below minPx and
// stays within the 0.15–0.85 hard band.
export function useDragRatio(
  containerRef: React.RefObject<HTMLElement | null>,
  { storageKey, axis, minPx = DEFAULT_MIN_PX, defaultRatio = DEFAULT_RATIO }: Options,
) {
  const [ratio, setRatio] = useState(() => loadRatio(storageKey, defaultRatio));
  const dragging = useRef(false);

  const clamp = useCallback(
    (r: number, extent: number) => {
      const bounded = Math.min(HARD_HI, Math.max(HARD_LO, r));
      if (extent <= minPx * 2) return Math.min(HARD_HI, Math.max(HARD_LO, defaultRatio));
      const lo = Math.max(HARD_LO, minPx / extent);
      const hi = Math.min(HARD_HI, 1 - minPx / extent);
      return Math.min(hi, Math.max(lo, bounded));
    },
    [minPx, defaultRatio],
  );

  const onPointerDown = useCallback((e: React.PointerEvent) => {
    e.preventDefault();
    dragging.current = true;
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }, []);

  useEffect(() => {
    const move = (e: PointerEvent) => {
      const box = containerRef.current;
      if (!dragging.current || !box) return;
      const rect = box.getBoundingClientRect();
      const r =
        axis === "x"
          ? (e.clientX - rect.left) / rect.width
          : (e.clientY - rect.top) / rect.height;
      setRatio(clamp(r, axis === "x" ? rect.width : rect.height));
    };
    const up = () => {
      if (!dragging.current) return;
      dragging.current = false;
      setRatio((r) => {
        safeSet(storageKey, String(r));
        return r;
      });
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
  }, [containerRef, clamp, axis, storageKey]);

  const reset = useCallback(() => {
    setRatio(defaultRatio);
    safeSet(storageKey, String(defaultRatio));
  }, [defaultRatio, storageKey]);

  return { ratio, onPointerDown, reset };
}
