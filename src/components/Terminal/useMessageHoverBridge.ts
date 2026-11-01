import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { withinMessageHoverArea } from "./messageHoverGeometry";

export function useMessageHoverIntent(onOpen: () => void) {
  const [pending, setPending] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const latest = useRef(onOpen);
  latest.current = onOpen;
  const cancel = useCallback(() => {
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = null;
    setPending(false);
  }, []);
  const start = useCallback(() => {
    if (timer.current !== null) return;
    setPending(true);
    timer.current = setTimeout(() => {
      timer.current = null;
      setPending(false);
      latest.current();
    }, 300);
  }, []);
  useEffect(() => () => { if (timer.current !== null) clearTimeout(timer.current); }, []);
  return { pending, start, cancel };
}

export function useMessageHoverBridge({ open, anchor, menu, isHover, onClose }: {
  open: boolean;
  anchor: RefObject<HTMLElement | null>;
  menu: RefObject<HTMLElement | null>;
  isHover: () => boolean;
  onClose: () => void;
}) {
  const closing = useRef<ReturnType<typeof setTimeout> | null>(null);
  const latest = useRef({ isHover, onClose });
  latest.current = { isHover, onClose };
  const cancelClose = useCallback(() => {
    if (closing.current !== null) clearTimeout(closing.current);
    closing.current = null;
  }, []);
  const onMove = (event: { pointerType: string; clientX: number; clientY: number }) => {
    if (event.pointerType !== "mouse" || !latest.current.isHover()) { cancelClose(); return; }
    const anchorBounds = anchor.current?.getBoundingClientRect();
    const menuBounds = menu.current?.getBoundingClientRect();
    if (anchorBounds && menuBounds && withinMessageHoverArea({ x: event.clientX, y: event.clientY }, anchorBounds, menuBounds)) {
      cancelClose();
    } else if (closing.current === null) {
      closing.current = setTimeout(() => {
        closing.current = null;
        if (latest.current.isHover()) latest.current.onClose();
      }, 300);
    }
  };
  const move = useRef(onMove);
  move.current = onMove;
  useEffect(() => {
    if (!open) return;
    const listener = (event: PointerEvent) => move.current(event);
    document.addEventListener("pointermove", listener, true);
    return () => { document.removeEventListener("pointermove", listener, true); cancelClose(); };
  }, [open, cancelClose]);
  return onMove;
}
