import type { ChatRowGeometry } from "./chatRowGeometry";

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
  return mountedChatWindowFromGeometry({ geometry: {
    count: heights.length,
    offset: (index) => offsets[index],
    boundary: (top, inclusive) => {
      let low = 0;
      let high = offsets.length;
      while (low < high) {
        const middle = Math.floor((low + high) / 2);
        if (inclusive ? offsets[middle] <= top : offsets[middle] < top) low = middle + 1;
        else high = middle;
      }
      return Math.max(0, low - 1);
    },
  }, scrollTop, viewportHeight, active, pinned, targetIndex });
}

export function mountedChatWindowFromGeometry({ geometry, scrollTop, viewportHeight, active = true, pinned = false, targetIndex }: {
  geometry: ChatRowGeometry;
  scrollTop: number;
  viewportHeight: number;
  active?: boolean;
  pinned?: boolean;
  targetIndex?: number;
}): MountedChatWindow {
  const count = geometry.count;
  const total = geometry.offset(count);
  if (!active || count === 0) return { start: 0, end: 0, top: 0, bottom: total, total };
  const height = Math.max(1, viewportHeight);
  const top = targetIndex === undefined
    ? pinned ? Math.max(0, total - height) : Math.max(0, Math.min(scrollTop, total - height))
    : geometry.offset(Math.max(0, Math.min(count - 1, targetIndex)));
  const visibleStart = Math.min(count - 1, geometry.boundary(top, true));
  const ceiling = (value: number) => {
    const floor = geometry.boundary(value, false);
    return geometry.offset(floor) >= value ? floor : Math.min(count, floor + 1);
  };
  const visibleEnd = Math.max(visibleStart + 1, ceiling(top + height));
  let start = Math.min(visibleStart, geometry.boundary(top - height, true));
  let end = Math.max(visibleEnd, ceiling(top + 2 * height));
  const limit = visibleEnd - visibleStart > CHAT_MOUNT_TARGET ? CHAT_MOUNT_MAX : CHAT_MOUNT_TARGET;
  if (end - start > limit) {
    const before = Math.floor(Math.max(0, limit - (visibleEnd - visibleStart)) / 2);
    start = Math.max(start, visibleStart - before);
    end = Math.min(count, Math.max(visibleEnd, start + limit));
  }
  return { start, end, top: geometry.offset(start), bottom: total - geometry.offset(end), total };
}

export function sameMountedChatWindow(left: MountedChatWindow, right: MountedChatWindow): boolean {
  return left.start === right.start && left.end === right.end && left.top === right.top
    && left.bottom === right.bottom && left.total === right.total;
}
