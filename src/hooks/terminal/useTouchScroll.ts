import { useEffect } from "react";
import type { Terminal } from "@xterm/xterm";

export interface UseTouchScrollOptions {
  containerRef: React.RefObject<HTMLDivElement | null>;
  termRef: React.MutableRefObject<Terminal | null>;
  termReady: boolean;
}

// xterm.js only scrolls via wheel events; touch swipes fall through to the
// scrollable ancestor (chat behind the fallback). Translate them ourselves.
export function useTouchScroll({ containerRef, termRef, termReady }: UseTouchScrollOptions): void {
  useEffect(() => {
    const el = containerRef.current;
    if (!el || !termReady) return;

    let lastY: number | null = null;
    let carry = 0;

    const onStart = (e: TouchEvent) => {
      if (e.touches.length !== 1) { lastY = null; return; }
      lastY = e.touches[0].clientY;
      carry = 0;
    };

    const onMove = (e: TouchEvent) => {
      const term = termRef.current;
      if (lastY == null || e.touches.length !== 1 || !term) return;
      const y = e.touches[0].clientY;
      const cell = el.clientHeight / Math.max(term.rows, 1);
      carry += (y - lastY) / cell;
      lastY = y;
      const lines = Math.trunc(carry);
      if (lines !== 0) {
        carry -= lines;
        if (term.buffer.active.type === "alternate") {
          // TUIs own their scroll — mimic xterm's wheel behavior with arrows.
          const key = lines > 0 ? "\x1b[A" : "\x1b[B";
          term.input(key.repeat(Math.abs(lines)));
        } else {
          term.scrollLines(-lines);
        }
      }
      e.preventDefault();
    };

    const onEnd = () => { lastY = null; };

    el.addEventListener("touchstart", onStart, { passive: true });
    el.addEventListener("touchmove", onMove, { passive: false });
    el.addEventListener("touchend", onEnd, { passive: true });
    el.addEventListener("touchcancel", onEnd, { passive: true });
    return () => {
      el.removeEventListener("touchstart", onStart);
      el.removeEventListener("touchmove", onMove);
      el.removeEventListener("touchend", onEnd);
      el.removeEventListener("touchcancel", onEnd);
    };
  }, [containerRef, termRef, termReady]);
}
