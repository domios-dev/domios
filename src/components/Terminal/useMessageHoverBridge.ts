import { useEffect, type RefObject } from "react";
import { withinMessageHoverArea } from "./messageHoverGeometry";

export function useMessageHoverBridge({ open, anchor, menu, isHover, onClose }: {
  open: boolean;
  anchor: RefObject<HTMLElement | null>;
  menu: RefObject<HTMLElement | null>;
  isHover: () => boolean;
  onClose: () => void;
}) {
  const onMove = (event: { pointerType: string; clientX: number; clientY: number }) => {
    if (event.pointerType !== "mouse" || !isHover()) return;
    const anchorBounds = anchor.current?.getBoundingClientRect();
    const menuBounds = menu.current?.getBoundingClientRect();
    if (!anchorBounds || !menuBounds || !withinMessageHoverArea({ x: event.clientX, y: event.clientY }, anchorBounds, menuBounds)) onClose();
  };
  useEffect(() => {
    if (!open) return;
    document.addEventListener("pointermove", onMove, true);
    return () => document.removeEventListener("pointermove", onMove, true);
  });
  return onMove;
}
