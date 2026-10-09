export interface TetherRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface TetherClearing {
  cx: number;
  cy: number;
  r: number;
}

/** Control-arm length as a fraction of each axis; below the quarter-ellipse 0.55 so the arc stays soft and never reads as a corner. */
export const TETHER_BEND = 0.5;
export const TETHER_GAP = 2;
/** The arc starts this far below the owner's glyph so it never touches a provider icon. */
export const TETHER_OWNER_GAP = 2.5;

const round = (value: number) => Math.round(value * 100) / 100;
const fmt = (value: number) => String(round(value));

function measurable(rect: TetherRect): boolean {
  return rect.width > 0 && rect.height > 0;
}

function centre(rect: TetherRect): [number, number] {
  return [rect.left + rect.width / 2, rect.top + rect.height / 2];
}

/** One cubic curve per attachment, from just below the owner icon to the attachment icon's centre; "" when it cannot be drawn. */
// Fanned curves read as tangled hair past one or two attachments; once owners hold several surfaces, tether to a single grouping tray instead.
export function tetherPaths(owner: TetherRect, attachments: readonly TetherRect[], bend = TETHER_BEND): string[] {
  if (!measurable(owner)) return attachments.map(() => "");
  const x0 = owner.left + owner.width / 2;
  const y0 = owner.top + owner.height + TETHER_OWNER_GAP;
  return attachments.map((attachment) => {
    if (!measurable(attachment)) return "";
    const [x1, y1] = centre(attachment);
    if (y1 <= y0) return "";
    const c1y = y0 + (y1 - y0) * bend;
    const c2x = x1 - (x1 - x0) * bend;
    return `M${fmt(x0)} ${fmt(y0)}C${fmt(x0)} ${fmt(c1y)} ${fmt(c2x)} ${fmt(y1)} ${fmt(x1)} ${fmt(y1)}`;
  });
}

/** Discs around each measured attachment icon that the strands are masked out of, so a curve never crosses its glyph. */
export function tetherClearings(attachments: readonly TetherRect[]): TetherClearing[] {
  return attachments.filter(measurable).map((rect) => {
    const [cx, cy] = centre(rect);
    return { cx: round(cx), cy: round(cy), r: round(Math.max(rect.width, rect.height) / 2 + TETHER_GAP) };
  });
}

/** `el`'s box in `container`'s coordinate space. */
export function relativeRect(el: Element, container: Element): TetherRect {
  const box = el.getBoundingClientRect();
  const origin = container.getBoundingClientRect();
  return { left: box.left - origin.left, top: box.top - origin.top, width: box.width, height: box.height };
}
