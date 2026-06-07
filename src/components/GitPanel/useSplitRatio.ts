import { useCallback, useEffect, useRef, useState } from "react";

const KEY = "ct-git-split";
const MIN_PX = 280;
const DEFAULT = 0.5;

function load(): number {
  if (typeof window === "undefined") return DEFAULT;
  const raw = Number(window.localStorage.getItem(KEY));
  return raw > 0.05 && raw < 0.95 ? raw : DEFAULT;
}

// Draggable vertical divider ratio (left-pane fraction), persisted, clamped so
// neither side drops below MIN_PX given the container's current width.
export function useSplitRatio(containerRef: React.RefObject<HTMLDivElement | null>) {
  const [ratio, setRatio] = useState(load);
  const dragging = useRef(false);

  const clamp = useCallback((r: number, width: number) => {
    if (width <= MIN_PX * 2) return DEFAULT;
    const lo = MIN_PX / width;
    return Math.min(1 - lo, Math.max(lo, r));
  }, []);

  const onPointerDown = useCallback(
    (e: React.PointerEvent) => {
      e.preventDefault();
      dragging.current = true;
      (e.target as HTMLElement).setPointerCapture(e.pointerId);
    },
    [],
  );

  useEffect(() => {
    const move = (e: PointerEvent) => {
      const box = containerRef.current;
      if (!dragging.current || !box) return;
      const rect = box.getBoundingClientRect();
      setRatio(clamp((e.clientX - rect.left) / rect.width, rect.width));
    };
    const up = () => {
      if (!dragging.current) return;
      dragging.current = false;
      setRatio((r) => {
        window.localStorage.setItem(KEY, String(r));
        return r;
      });
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
  }, [containerRef, clamp]);

  const reset = useCallback(() => {
    setRatio(DEFAULT);
    window.localStorage.setItem(KEY, String(DEFAULT));
  }, []);

  return { ratio, onPointerDown, reset };
}
