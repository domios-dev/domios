import type { ViewportBox } from "@codeterm/canvas-camera";

export function layoutGallery(sizes: { width: number; height: number }[], gap = 40): { items: ViewportBox[]; bounds: ViewportBox } {
  let x = 0;
  const height = Math.max(1, ...sizes.map(size => size.height));
  const items = sizes.map(size => {
    const box = { x, y: (height - size.height) / 2, ...size };
    x += size.width + gap;
    return box;
  });
  return { items, bounds: { x: 0, y: 0, width: Math.max(1, x - gap), height } };
}

export function galleryNext(index: number, direction: number, count: number): number {
  return count > 0 ? ((index + direction) % count + count) % count : 0;
}

export function swipeDirection(dx: number, dy: number, duration: number, pinched: boolean): number {
  return !pinched && duration < 500 && Math.abs(dx) > 70 && Math.abs(dx) > Math.abs(dy) * 2 ? (dx < 0 ? 1 : -1) : 0;
}
