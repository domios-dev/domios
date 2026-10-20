export type FloatingPlacement = `${"top" | "bottom"}-${"start" | "center" | "end"}`;

export interface AnchorRect {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

export interface FloatingViewport {
  width: number;
  height: number;
  offsetLeft?: number;
  offsetTop?: number;
}

export interface FloatingPosition {
  top: number;
  left: number;
  maxHeight: number;
  maxWidth: number;
  side: "top" | "bottom";
  align: "start" | "center" | "end";
}

export interface FloatingSpacing {
  margin?: number;
  gap?: number;
}

/** Below this much room on either side a popover may overlap its anchor rather than shrink to nothing. */
const MIN_USEFUL_HEIGHT = 96;

export function pointAnchor(x: number, y: number): AnchorRect {
  return { left: x, right: x, top: y, bottom: y };
}

export function anchoredPopoverPosition(
  anchor: AnchorRect,
  size: { width: number; height: number },
  viewport: FloatingViewport,
  placement: FloatingPlacement = "bottom-start",
  { margin = 8, gap = 4 }: FloatingSpacing = {},
): FloatingPosition {
  const leftEdge = (viewport.offsetLeft ?? 0) + margin;
  const topEdge = (viewport.offsetTop ?? 0) + margin;
  const rightEdge = (viewport.offsetLeft ?? 0) + viewport.width - margin;
  const bottomEdge = (viewport.offsetTop ?? 0) + viewport.height - margin;
  const maxWidth = Math.max(0, rightEdge - leftEdge);
  const screenHeight = Math.max(0, bottomEdge - topEdge);

  const belowRoom = bottomEdge - (anchor.bottom + gap);
  const aboveRoom = anchor.top - gap - topEdge;
  const preferred = placement.startsWith("top") ? "top" : "bottom";
  const preferredRoom = preferred === "top" ? aboveRoom : belowRoom;
  const otherRoom = preferred === "top" ? belowRoom : aboveRoom;
  const side = preferredRoom >= size.height || preferredRoom >= otherRoom
    ? preferred
    : preferred === "top" ? "bottom" : "top";
  const room = Math.max(side === preferred ? preferredRoom : otherRoom, Math.min(size.height, MIN_USEFUL_HEIGHT));
  const maxHeight = Math.max(0, Math.min(size.height, room, screenHeight));
  const anchoredTop = side === "top" ? anchor.top - gap - maxHeight : anchor.bottom + gap;
  const top = Math.max(topEdge, Math.min(anchoredTop, bottomEdge - maxHeight));

  const width = Math.min(size.width, maxWidth);
  const align = placement.endsWith("end") ? "end" : placement.endsWith("center") ? "center" : "start";
  const anchoredLeft = align === "start" ? anchor.left
    : align === "end" ? anchor.right - width
    : (anchor.left + anchor.right - width) / 2;
  const left = Math.max(leftEdge, Math.min(anchoredLeft, rightEdge - width));
  return { top, left, maxHeight, maxWidth, side, align };
}
