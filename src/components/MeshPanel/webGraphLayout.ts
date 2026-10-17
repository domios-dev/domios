export const WEB_CARD_SCALE = 0.8;
export const MACHINE_CARD_SCALE = 1.5;
export const MACHINE_CARD_WIDTH = 360 * MACHINE_CARD_SCALE;
export const MACHINE_CARD_HEIGHT = 340 * MACHINE_CARD_SCALE;
const GAP = 20;

export function webGraphLayout(width: number, height: number, machineCount: number, webHeight = 800) {
  const originalWidth = Math.min(344, Math.max(0, width - 32));
  const webWidth = originalWidth * WEB_CARD_SCALE;
  const qrSize = Math.max(0, Math.min(280, originalWidth - 64)) * WEB_CARD_SCALE;
  const directions = Array.from({ length: machineCount }, (_, index) => {
    const angle = index / machineCount * Math.PI * 2 - Math.PI / 2;
    return { x: Math.cos(angle), y: Math.sin(angle) };
  });
  const exitRadius = (x: number, y: number, halfWidth: number, halfHeight: number) => Math.min(
    Math.abs(x) < 1e-10 ? Infinity : halfWidth / Math.abs(x),
    Math.abs(y) < 1e-10 ? Infinity : halfHeight / Math.abs(y),
  );
  let radius = 0;
  for (const [index, direction] of directions.entries()) {
    radius = Math.max(radius, exitRadius(direction.x, direction.y, (webWidth + MACHINE_CARD_WIDTH) / 2 + GAP, (webHeight + MACHINE_CARD_HEIGHT) / 2 + GAP));
    for (const other of directions.slice(index + 1)) {
      radius = Math.max(radius, exitRadius(direction.x - other.x, direction.y - other.y, MACHINE_CARD_WIDTH + GAP, MACHINE_CARD_HEIGHT + GAP));
    }
  }
  const center = { x: width / 2, y: height / 2 };
  return {
    web: { ...center, width: webWidth, height: webHeight },
    qrSize, radius,
    machines: directions.map((direction) => ({ x: center.x + direction.x * radius, y: center.y + direction.y * radius })),
  };
}
type Bounds = { x: number; y: number; width: number; height: number };

export function webGraphInitialView(width: number, height: number, bounds: Bounds, web: { x: number; y: number; width: number; height: number }, qrSize: number) {
  const readableScale = Math.max(200 / Math.max(1, qrSize), 8 / (11 * MACHINE_CARD_SCALE));
  const fitScale = Math.min(width / bounds.width, height / bounds.height, 1);
  if (fitScale >= readableScale) {
    const frame = { width: width / fitScale, height: height / fitScale };
    return { mode: "fit" as const, scale: fitScale, bounds: { x: bounds.x + bounds.width / 2 - frame.width / 2, y: bounds.y + bounds.height / 2 - frame.height / 2, ...frame } };
  }
  const scale = Math.max(readableScale, Math.min(1, width / web.width, height / web.height));
  const frame = { width: width / scale, height: height / scale };
  return { mode: "web" as const, scale, bounds: { x: web.x - frame.width / 2, y: web.y - frame.height / 2, ...frame } };
}
