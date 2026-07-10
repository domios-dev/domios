import { useCallback, useRef, type PointerEvent as ReactPointerEvent } from "react";

export const DRAG_CLICK_THRESHOLD_PX = 4;

export type PlanTaskPointerState = { x: number; y: number; dragged: boolean };

export function hasTextSelection(): boolean {
  if (typeof window === "undefined") return false;
  const sel = window.getSelection();
  if (!sel || sel.isCollapsed) return false;
  return sel.toString().length > 0;
}

export function markPointerDown(state: PlanTaskPointerState, clientX: number, clientY: number): void {
  state.x = clientX;
  state.y = clientY;
  state.dragged = false;
}

export function markPointerMove(state: PlanTaskPointerState, clientX: number, clientY: number): void {
  if (state.dragged) return;
  const dx = clientX - state.x;
  const dy = clientY - state.y;
  if (Math.hypot(dx, dy) > DRAG_CLICK_THRESHOLD_PX) state.dragged = true;
}

export function shouldSuppressPlanTaskActivate(state: PlanTaskPointerState): boolean {
  return state.dragged || hasTextSelection();
}

export function usePlanTaskActivate(onActivate?: () => void) {
  const ptr = useRef<PlanTaskPointerState>({ x: 0, y: 0, dragged: false });

  const onPointerDown = useCallback((e: ReactPointerEvent) => {
    markPointerDown(ptr.current, e.clientX, e.clientY);
  }, []);

  const onPointerMove = useCallback((e: ReactPointerEvent) => {
    markPointerMove(ptr.current, e.clientX, e.clientY);
  }, []);

  const onClick = useCallback(() => {
    if (!onActivate || shouldSuppressPlanTaskActivate(ptr.current)) return;
    onActivate();
  }, [onActivate]);

  return { onPointerDown, onPointerMove, onClick };
}
