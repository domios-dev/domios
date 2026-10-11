import { useEffect } from "react";
import type { Terminal } from "@xterm/xterm";
import { dispatchTerminalTouchScroll } from "./terminalScroll";

export interface UseTouchScrollOptions {
  containerRef: React.RefObject<HTMLDivElement | null>;
  termRef: React.MutableRefObject<Terminal | null>;
  termReady: boolean;
}

export function useTouchScroll({ containerRef, termRef, termReady }: UseTouchScrollOptions): void {
  useEffect(() => {
    const el = containerRef.current;
    if (!el || !termReady) return;

    const previousTouchAction = el.style.touchAction;
    el.style.touchAction = "none";

    let lastY: number | null = null;
    let lastX = 0;
    let carry = 0;

    const onStart = (e: TouchEvent) => {
      if (e.touches.length !== 1) { lastY = null; return; }
      lastY = e.touches[0].clientY;
      lastX = e.touches[0].clientX;
      carry = 0;
    };

    const onMove = (e: TouchEvent) => {
      const term = termRef.current;
      if (lastY == null || e.touches.length !== 1 || !term || el.clientHeight === 0) return;
      const y = e.touches[0].clientY;
      const x = e.touches[0].clientX;
      const start = { x: lastX, y: lastY };
      const cell = el.clientHeight / Math.max(term.rows, 1);
      carry += (y - lastY) / cell;
      lastY = y;
      lastX = x;
      const lines = Math.trunc(carry);
      if (lines !== 0) {
        carry -= lines;
        dispatchTerminalTouchScroll(term, -lines, x, y, start);
      }
      e.preventDefault();
      e.stopPropagation();
    };

    const onEnd = () => { lastY = null; };

    el.addEventListener("touchstart", onStart, { passive: true });
    el.addEventListener("touchmove", onMove, { passive: false, capture: true });
    el.addEventListener("touchend", onEnd, { passive: true });
    el.addEventListener("touchcancel", onEnd, { passive: true });
    return () => {
      el.style.touchAction = previousTouchAction;
      el.removeEventListener("touchstart", onStart);
      el.removeEventListener("touchmove", onMove, true);
      el.removeEventListener("touchend", onEnd);
      el.removeEventListener("touchcancel", onEnd);
    };
  }, [containerRef, termRef, termReady]);
}
