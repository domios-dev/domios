export function tabResizeChanged(last: { w: number; h: number }, w: number, h: number): boolean {
  return w >= 20 && h >= 20 && (w !== last.w || h !== last.h);
}
