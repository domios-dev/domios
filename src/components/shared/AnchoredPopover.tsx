import { useEffect, useRef, type CSSProperties, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import type { FloatingPlacement } from "./floatingPosition";
import { useAnchoredPopoverPosition, type PopoverAnchor } from "./useAnchoredPopoverPosition";

export interface AnchoredPopoverProps {
  open: boolean;
  /** The trigger element, or a viewport point for context menus and selections. */
  anchor: PopoverAnchor;
  placement?: FloatingPlacement;
  /** Space between anchor and popover; hover popovers pass 0 so the pointer never crosses a gap. */
  gap?: number;
  children: ReactNode;
  className?: string;
  style?: CSSProperties;
  width?: CSSProperties["width"] | "anchor";
  /** Outside pointer presses and an unhandled Escape call it; Escape returns focus to the anchor. */
  onClose?: () => void;
  onPointerEnter?: (event: React.PointerEvent<HTMLDivElement>) => void;
  onPointerLeave?: (event: React.PointerEvent<HTMLDivElement>) => void;
  floatingRef?: RefObject<HTMLDivElement | null>;
}

/** The one way to float content next to an anchor: portaled, flipped and clamped inside the visual viewport. */
export function AnchoredPopover({
  open,
  anchor,
  placement = "bottom-start",
  gap,
  children,
  className = "",
  style,
  width,
  onClose,
  onPointerEnter,
  onPointerLeave,
  floatingRef,
}: AnchoredPopoverProps) {
  const ownRef = useRef<HTMLDivElement>(null);
  const contentRef = floatingRef ?? ownRef;
  const pressedInside = useRef(false);
  const position = useAnchoredPopoverPosition(open, anchor, contentRef, placement, { gap });
  const anchorElement = "current" in anchor ? anchor : null;

  useEffect(() => {
    if (!open || !onClose) return;
    const outside = (event: PointerEvent) => {
      if (pressedInside.current || (event.target instanceof Node && anchorElement?.current?.contains(event.target))) return;
      onClose();
    };
    const escape = (event: KeyboardEvent) => {
      if ((event.code !== "Escape" && event.key !== "Escape") || event.defaultPrevented) return;
      event.preventDefault();
      const active = document.activeElement;
      const focusInside = !active || active === document.body || Boolean(contentRef.current?.contains(active));
      onClose();
      if (focusInside) anchorElement?.current?.focus({ preventScroll: true });
    };
    const reset = () => { pressedInside.current = false; };
    document.addEventListener("pointerdown", reset, true);
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("pointerdown", reset, true);
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("keydown", escape);
    };
  }, [open, onClose, anchorElement, contentRef]);

  if (!open) return null;
  const vertical = position?.side === "top" ? "bottom" : "top";
  const horizontal = position?.align === "end" ? "right" : position?.align === "center" ? "center" : "left";
  return createPortal(
    <div
      ref={contentRef}
      data-anchored-popover=""
      data-side={position?.side}
      onPointerDownCapture={() => { pressedInside.current = true; }}
      onPointerEnter={onPointerEnter}
      onPointerLeave={onPointerLeave}
      className={`ct-anchored-popover ${className}`.trim()}
      style={{
        top: position?.top ?? 0,
        left: position?.left ?? 0,
        width: width === "anchor" ? anchorElement?.current?.getBoundingClientRect().width : width ?? "max-content",
        maxWidth: position?.maxWidth,
        maxHeight: position?.maxHeight,
        opacity: position ? undefined : 0,
        "--ct-menu-origin": `${vertical} ${horizontal}`,
        "--ct-menu-shift": position?.side === "top" ? "6px" : "-6px",
        ...style,
      } as CSSProperties}
    >
      {children}
    </div>,
    document.body,
  );
}
