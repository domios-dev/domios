export function chatFloatingBottomInset(bottomBarTop: number, floatingTops: readonly number[]): number {
  return Math.ceil(Math.max(0, ...floatingTops.map((top) => bottomBarTop - top)));
}
