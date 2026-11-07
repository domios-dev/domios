export interface ToastObstacle { left: number; right: number; top: number; bottom: number; width: number; height: number }
export function toastStackTop(host: Pick<ToastObstacle, "left" | "right">, obstacles: readonly ToastObstacle[]): number {
  return obstacles.reduce((bottom, rect) => rect.width > 0 && rect.height > 0 && rect.right > host.left && rect.left < host.right
    ? Math.max(bottom, rect.bottom) : bottom, 0) + 6;
}
