export function orderedListGutterCh(start: number, count: number, reversed = false): number {
  const first = Number.isFinite(start) ? Math.trunc(start) : 1;
  const last = first + Math.max(0, count - 1) * (reversed ? -1 : 1);
  return Math.max(String(first).length, String(last).length) + 1.5;
}
