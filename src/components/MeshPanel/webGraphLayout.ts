export const WEB_CARD_SCALE = 0.8;
export const MACHINE_CARD_SCALE = 1;
export const MACHINE_CARD_WIDTH = 360 * MACHINE_CARD_SCALE;
export const MACHINE_CARD_HEIGHT = 260 * MACHINE_CARD_SCALE;
const GAP = 64;

export function webGraphLayout(width: number, height: number, machineCount: number, webHeight = 800, machineHeights: number[] = []) {
  const originalWidth = Math.min(280, Math.max(0, width - 32));
  const webWidth = originalWidth * WEB_CARD_SCALE;
  const qrSize = Math.max(0, Math.min(160, originalWidth - 48)) * WEB_CARD_SCALE;
  const directions = Array.from({ length: machineCount }, (_, index) => {
    const angle = index / machineCount * Math.PI * 2 - Math.PI / 2;
    return { x: Math.cos(angle), y: Math.sin(angle) };
  });
  const exitRadius = (x: number, y: number, halfWidth: number, halfHeight: number) => Math.min(
    Math.abs(x) < 1e-10 ? Infinity : halfWidth / Math.abs(x),
    Math.abs(y) < 1e-10 ? Infinity : halfHeight / Math.abs(y),
  );
  const heights = directions.map((_, index) => machineHeights[index] ?? MACHINE_CARD_HEIGHT);
  const place = (gap: number) => directions.map((direction, index) => {
    const radius = exitRadius(direction.x, direction.y, (webWidth + MACHINE_CARD_WIDTH) / 2 + gap, (webHeight + heights[index]) / 2 + gap);
    return { x: direction.x * radius, y: direction.y * radius };
  });
  let gap = GAP;
  let positions = place(gap);
  while (positions.some((point, index) => positions.slice(index + 1).some((other, offset) =>
    Math.abs(point.x - other.x) < MACHINE_CARD_WIDTH + GAP && Math.abs(point.y - other.y) < (heights[index] + heights[index + offset + 1]) / 2 + GAP))) {
    gap += GAP;
    positions = place(gap);
  }

  const center = { x: width / 2, y: height / 2 };
  return {
    web: { ...center, width: webWidth, height: webHeight },
    qrSize, gap,
    machines: positions.map((point, index) => ({ x: center.x + point.x, y: center.y + point.y, height: heights[index] })),
  };
}
type Bounds = { x: number; y: number; width: number; height: number };

export function webGraphInitialView(width: number, height: number, bounds: Bounds, userAdjusted = false) {
  if (userAdjusted || width <= 0 || height <= 0 || bounds.width <= 0 || bounds.height <= 0) return null;
  const scale = Math.min(width / bounds.width, height / bounds.height, 1);
  return { scale, bounds };
}
