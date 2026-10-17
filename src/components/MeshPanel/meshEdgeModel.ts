import { bezierPath } from "../Canvas/edges";

type Card = { x: number; y: number; width: number; height: number };

export function meshEdgeModel(from: Card, to: Card, online: boolean) {
  const dx = to.x - from.x;
  const dy = to.y - from.y;
  const vertical = Math.abs(dy) > Math.abs(dx);
  const anchor = (card: Card, direction: number) => vertical
    ? { x: card.x, y: card.y + direction * Math.sign(dy) * card.height / 2 }
    : { x: card.x + direction * Math.sign(dx) * card.width / 2, y: card.y };
  const a = anchor(from, 1);
  const b = anchor(to, -1);
  if (vertical) { a.x -= 12; b.x += 12; }
  else { a.y -= 12; b.y += 12; }
  const path = vertical
    ? `M ${a.x} ${a.y} C ${a.x} ${a.y + Math.sign(dy) * Math.max(Math.abs(b.y - a.y) * 0.35, 24)}, ${b.x} ${b.y - Math.sign(dy) * Math.max(Math.abs(b.y - a.y) * 0.35, 24)}, ${b.x} ${b.y}`
    : bezierPath(a, b, 0.35);
  return { path, opacity: online ? 0.36 : 0.18, dash: online ? undefined : "5 6" };
}
