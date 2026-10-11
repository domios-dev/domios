import { useLayoutEffect, useState, type RefObject } from "react";
import { anchoredFloatingPosition, type FloatingPosition } from "./floatingPosition";

export function useAnchoredFloatingPosition(
  open: boolean,
  triggerRef: RefObject<HTMLElement | null>,
  menuRef: RefObject<HTMLElement | null>,
  width: number,
  align: "left" | "right",
): FloatingPosition | null {
  const [position, setPosition] = useState<FloatingPosition | null>(null);
  const mounted = position !== null;
  useLayoutEffect(() => {
    if (!open) return;
    const update = () => {
      const trigger = triggerRef.current;
      const menu = menuRef.current;
      if (!trigger) return;
      setPosition(anchoredFloatingPosition(
        trigger.getBoundingClientRect(), menu?.offsetWidth || width,
        menu?.scrollHeight || menu?.offsetHeight || 0, window.innerWidth, window.innerHeight, align,
      ));
    };
    update();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(update);
    if (menuRef.current) observer?.observe(menuRef.current);
    window.addEventListener("resize", update);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", update);
    };
  }, [open, triggerRef, menuRef, width, align, mounted]);
  return position;
}
