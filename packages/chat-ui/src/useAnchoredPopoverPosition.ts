import { useLayoutEffect, useState, type RefObject } from "react";
import {
  anchoredPopoverPosition,
  pointAnchor,
  type FloatingPlacement,
  type FloatingPosition,
  type FloatingSpacing,
  type FloatingViewport,
} from "./floatingPosition";

export type PopoverAnchor = RefObject<HTMLElement | null> | { x: number; y: number };

export function visualViewportBounds(): FloatingViewport {
  const visual = window.visualViewport;
  return visual
    ? { width: visual.width, height: visual.height, offsetLeft: visual.offsetLeft, offsetTop: visual.offsetTop }
    : { width: window.innerWidth, height: window.innerHeight };
}

function samePosition(a: FloatingPosition | null, b: FloatingPosition): boolean {
  return a !== null && a.top === b.top && a.left === b.left && a.maxHeight === b.maxHeight
    && a.maxWidth === b.maxWidth && a.side === b.side && a.align === b.align;
}

/** Natural size, unconstrained by the max-height a previous placement applied. */
function naturalSize(floating: HTMLElement): { width: number; height: number } {
  const previous = floating.style.maxHeight;
  floating.style.maxHeight = "none";
  const size = { width: floating.offsetWidth, height: floating.offsetHeight };
  floating.style.maxHeight = previous;
  return size;
}

export function useAnchoredPopoverPosition(
  open: boolean,
  anchor: PopoverAnchor,
  floatingRef: RefObject<HTMLElement | null>,
  placement: FloatingPlacement,
  spacing: FloatingSpacing = {},
): FloatingPosition | null {
  const [position, setPosition] = useState<FloatingPosition | null>(null);
  const anchorRef = "current" in anchor ? anchor : null;
  const pointX = "current" in anchor ? null : anchor.x;
  const pointY = "current" in anchor ? null : anchor.y;
  const { margin, gap, keepSide } = spacing;
  useLayoutEffect(() => {
    if (!open) {
      setPosition(null);
      return;
    }
    const update = () => {
      const floating = floatingRef.current;
      const element = anchorRef?.current;
      const rect = element ? element.getBoundingClientRect() : pointX !== null && pointY !== null ? pointAnchor(pointX, pointY) : null;
      if (!floating || !rect) return;
      const next = anchoredPopoverPosition(rect, naturalSize(floating), visualViewportBounds(), placement, { margin, gap, keepSide });
      setPosition((current) => (samePosition(current, next) ? current : next));
    };
    update();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(update);
    if (anchorRef?.current) observer?.observe(anchorRef.current);
    // A child capped by the popover's own max-height never resizes as content arrives, so content changes re-measure too.
    const content = typeof MutationObserver === "undefined" ? null : new MutationObserver(update);
    if (floatingRef.current) {
      observer?.observe(floatingRef.current);
      for (const child of Array.from(floatingRef.current.children)) observer?.observe(child);
      content?.observe(floatingRef.current, { childList: true, subtree: true, characterData: true });
    }
    const visual = window.visualViewport;
    window.addEventListener("resize", update);
    window.addEventListener("scroll", update, true);
    visual?.addEventListener("resize", update);
    visual?.addEventListener("scroll", update);
    return () => {
      observer?.disconnect();
      content?.disconnect();
      window.removeEventListener("resize", update);
      window.removeEventListener("scroll", update, true);
      visual?.removeEventListener("resize", update);
      visual?.removeEventListener("scroll", update);
    };
  }, [open, anchorRef, pointX, pointY, floatingRef, placement, margin, gap, keepSide]);
  return open ? position : null;
}
