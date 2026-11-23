import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import type { ViewportTransform } from "./viewportModel";
import type { PanButton } from "./useCanvasViewport";

const isEditableTarget = (target: EventTarget | null) =>
  target instanceof Element && Boolean(target.closest("input, textarea, select, [contenteditable='true']"));

export function useCanvasMouse(transformRef: RefObject<ViewportTransform>, cancelAnim: () => void, schedule: (next: ViewportTransform) => void, panButton: PanButton, ignoreSelector?: string) {
  const [isDragging, setIsDragging] = useState(false);
  const [isSpaceHeld, setIsSpaceHeld] = useState(false);
  const spaceRef = useRef(false);
  const dragRef = useRef<{ startX: number; startY: number; baseX: number; baseY: number } | null>(null);
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.code !== "Space" || isEditableTarget(e.target) || isEditableTarget(document.activeElement)) return;
      if (!spaceRef.current) {
        spaceRef.current = true;
        setIsSpaceHeld(true);
      }
    };
    const releaseSpace = () => {
      if (!spaceRef.current) return;
      spaceRef.current = false;
      setIsSpaceHeld(false);
    };
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.code === "Space") releaseSpace();
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", releaseSpace);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", releaseSpace);
    };
  }, []);

  const onMouseDown = useCallback(
    (e: React.MouseEvent) => {
      const middle = e.button === 1;
      const spaceLeft = e.button === 0 && spaceRef.current;
      const allowed = middle || spaceLeft || panButton === "any";
      if (!allowed) return;
      if (ignoreSelector && (e.target as HTMLElement).closest(ignoreSelector)) return;
      cancelAnim();
      dragRef.current = {
        startX: e.clientX,
        startY: e.clientY,
        baseX: transformRef.current.x,
        baseY: transformRef.current.y,
      };
      setIsDragging(true);
    },
    [panButton, ignoreSelector, cancelAnim],
  );

  useEffect(() => {
    if (!isDragging) return;
    const onMove = (e: MouseEvent) => {
      const d = dragRef.current;
      if (!d) return;
      schedule({ ...transformRef.current, x: d.baseX + (e.clientX - d.startX), y: d.baseY + (e.clientY - d.startY) });
    };
    const onUp = () => {
      dragRef.current = null;
      setIsDragging(false);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, [isDragging, schedule]);

  return { isDragging, isSpaceHeld, onMouseDown };
}
