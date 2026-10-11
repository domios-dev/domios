export const CHAT_MOUNT_TARGET = 40;
export const CHAT_MOUNT_MAX = 60;
export const CHAT_ROW_ESTIMATE = 120;

export interface MountedChatWindow {
  start: number;
  end: number;
  top: number;
  bottom: number;
  total: number;
}

export function chatRowOffsets(heights: readonly number[]): number[] {
  const offsets = [0];
  for (const height of heights) offsets.push(offsets[offsets.length - 1] + (Number.isFinite(height) && height > 0 ? height : CHAT_ROW_ESTIMATE));
  return offsets;
}

export function mountedChatWindow({ heights, scrollTop, viewportHeight, active = true, pinned = false, targetIndex }: {
  heights: readonly number[];
  scrollTop: number;
  viewportHeight: number;
  active?: boolean;
  pinned?: boolean;
  targetIndex?: number;
}): MountedChatWindow {
  const offsets = chatRowOffsets(heights);
  const count = heights.length;
  const total = offsets[count];
  if (!active || count === 0) return { start: 0, end: 0, top: 0, bottom: total, total };
  const height = Math.max(1, viewportHeight);
  const top = targetIndex === undefined
    ? pinned ? Math.max(0, total - height) : Math.max(0, Math.min(scrollTop, total - height))
    : offsets[Math.max(0, Math.min(count - 1, targetIndex))];
  let visibleStart = 0;
  while (visibleStart < count - 1 && offsets[visibleStart + 1] <= top) visibleStart += 1;
  let visibleEnd = visibleStart + 1;
  while (visibleEnd < count && offsets[visibleEnd] < top + height) visibleEnd += 1;
  let start = visibleStart;
  while (start > 0 && offsets[start] > top - height) start -= 1;
  let end = visibleEnd;
  while (end < count && offsets[end] < top + 2 * height) end += 1;
  const limit = visibleEnd - visibleStart > CHAT_MOUNT_TARGET ? CHAT_MOUNT_MAX : CHAT_MOUNT_TARGET;
  if (end - start > limit) {
    const before = Math.floor(Math.max(0, limit - (visibleEnd - visibleStart)) / 2);
    start = Math.max(start, visibleStart - before);
    end = Math.min(count, start + limit);
  }
  return { start, end, top: offsets[start], bottom: total - offsets[end], total };
}
