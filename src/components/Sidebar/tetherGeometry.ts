export interface TetherRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export const TETHER_RADIUS = 6;
export const TETHER_GAP = 2;

const fmt = (value: number) => String(Math.round(value * 100) / 100);

function measurable(rect: TetherRect): boolean {
  return rect.width > 0 && rect.height > 0;
}

/** One SVG path per attachment, from under the owner's icon down and across to the attachment's icon; "" when it cannot be drawn. */
export function tetherPaths(owner: TetherRect, attachments: readonly TetherRect[], radius = TETHER_RADIUS): string[] {
  if (!measurable(owner)) return attachments.map(() => "");
  const x0 = owner.left + owner.width / 2;
  const y0 = owner.top + owner.height + TETHER_GAP;
  return attachments.map((attachment) => {
    if (!measurable(attachment)) return "";
    const x1 = attachment.left - TETHER_GAP;
    const y1 = attachment.top + attachment.height / 2;
    if (y1 <= y0) return "";
    if (x1 - x0 < 1) {
      const end = attachment.top - TETHER_GAP;
      return end > y0 ? `M${fmt(x0)} ${fmt(y0)}V${fmt(end)}` : "";
    }
    const r = Math.min(radius, y1 - y0, x1 - x0);
    return `M${fmt(x0)} ${fmt(y0)}V${fmt(y1 - r)}Q${fmt(x0)} ${fmt(y1)} ${fmt(x0 + r)} ${fmt(y1)}H${fmt(x1)}`;
  });
}

/** `el`'s box in `container`'s coordinate space. */
export function relativeRect(el: Element, container: Element): TetherRect {
  const box = el.getBoundingClientRect();
  const origin = container.getBoundingClientRect();
  return { left: box.left - origin.left, top: box.top - origin.top, width: box.width, height: box.height };
}
