import { useEffect, useRef, type CSSProperties, type ReactNode, type RefObject } from "react";
import { createPortal } from "react-dom";
import type { FloatingPlacement } from "./floatingPosition";
import { useAnchoredPopoverPosition, type PopoverAnchor } from "./useAnchoredPopoverPosition";
import { dismissOnEscape } from "./useEscapeDismissal";

export interface AnchoredPopoverProps {
  open: boolean;
  /** The trigger element, or a viewport point for context menus and selections. */
  anchor: PopoverAnchor;
  positionAnchor?: PopoverAnchor;
  placement?: FloatingPlacement;
  /** Space between anchor and popover; hover popovers pass 0 so the pointer never crosses a gap. */
  gap?: number;
  keepSide?: boolean;
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

function inAnyPopover(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest("[data-anchored-popover]") !== null;
}

/** The one way to float content next to an anchor: portaled, flipped and clamped inside the visual viewport. */
export function AnchoredPopover({
  open,
  anchor,
  positionAnchor,
  placement = "bottom-start",
  gap,
  keepSide,
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
  const position = useAnchoredPopoverPosition(open, positionAnchor ?? anchor, contentRef, placement, { gap, keepSide });
  const anchorElement = "current" in anchor ? anchor : null;

  // Capture phase: triggers often sit in containers that stop pointer and key propagation.
  // Presses and keys inside any popover belong to it, so a nested popover never closes its parent.
  useEffect(() => {
    if (!open || !onClose) return;
    const outside = (event: PointerEvent) => {
      if (inAnyPopover(event.target) || (event.target instanceof Node && anchorElement?.current?.contains(event.target))) return;
      onClose();
    };
    const escape = (event: KeyboardEvent) => {
      if (inAnyPopover(event.target)) return;
      const active = document.activeElement;
      const returnFocus = !active || active === document.body || Boolean(contentRef.current?.contains(active));
      if (dismissOnEscape(event, onClose) && returnFocus) anchorElement?.current?.focus({ preventScroll: true });
    };
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("keydown", escape, true);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("keydown", escape, true);
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
      onKeyDown={(event) => {
        if (onClose && dismissOnEscape(event, onClose)) anchorElement?.current?.focus({ preventScroll: true });
      }}
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
    document.fullscreenElement ?? document.body,
  );
}
