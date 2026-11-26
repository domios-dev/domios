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

export interface ChatScrollVelocityState {
  scrollTop: number;
  timestampMs: number;
  velocity: number;
  frameDelta: number;
}

const VELOCITY_RESET_MS = 120;
const VELOCITY_FRAME_MIN_MS = 8;

/** Tracks signed per-frame scroll velocity (px/frame) with instant attack and smooth decay. */
export function nextChatScrollVelocity(
  previous: ChatScrollVelocityState | null,
  scrollTop: number,
  timestampMs: number,
): ChatScrollVelocityState {
  if (!Number.isFinite(scrollTop) || !Number.isFinite(timestampMs)) {
    return previous ?? { scrollTop: 0, timestampMs: 0, velocity: 0, frameDelta: 0 };
  }
  if (!previous) {
    return { scrollTop, timestampMs, velocity: 0, frameDelta: 0 };
  }
  const delta = scrollTop - previous.scrollTop;
  const dt = timestampMs - previous.timestampMs;
  if (delta === 0) {
    if (dt >= VELOCITY_FRAME_MIN_MS || dt < 0) {
      return { scrollTop, timestampMs, velocity: 0, frameDelta: 0 };
    }
    return { ...previous, timestampMs };
  }
  if (dt < 0 || dt >= VELOCITY_RESET_MS) {
    return { scrollTop, timestampMs, velocity: delta, frameDelta: delta };
  }
  if (dt < VELOCITY_FRAME_MIN_MS) {
    const frameDelta = previous.frameDelta + delta;
    const sameDir = previous.velocity === 0 || Math.sign(frameDelta) === Math.sign(previous.velocity);
    const velocity = sameDir
      ? Math.sign(frameDelta) * Math.max(Math.abs(frameDelta), Math.abs(previous.velocity))
      : frameDelta;
    return { scrollTop, timestampMs: previous.timestampMs, velocity, frameDelta };
  }
  const sameDir = previous.velocity === 0 || Math.sign(delta) === Math.sign(previous.velocity);
  const velocity = sameDir
    ? Math.sign(delta) * Math.max(Math.abs(delta), Math.round(Math.abs(previous.velocity) * 0.75))
    : delta;
  return { scrollTop, timestampMs, velocity, frameDelta: delta };
}

function effectiveScrollVelocity(velocity: number | undefined, viewportHeight: number): number {
  if (velocity === undefined || !Number.isFinite(velocity)) return 0;
  return Math.abs(velocity) > viewportHeight * 0.25 ? velocity : 0;
}

export function chatRowOffsets(heights: readonly number[]): number[] {
  const offsets = [0];
  for (const height of heights) offsets.push(offsets[offsets.length - 1] + (Number.isFinite(height) && height >= 0 ? height : CHAT_ROW_ESTIMATE));
  return offsets;
}

export function mountedChatWindow({ heights, scrollTop, viewportHeight, active = true, pinned = false, targetIndex, velocity }: {
  heights: readonly number[];
  scrollTop: number;
  viewportHeight: number;
  active?: boolean;
  pinned?: boolean;
  targetIndex?: number;
  velocity?: number;
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
  }, scrollTop, viewportHeight, active, pinned, targetIndex, velocity });
}

export function mountedChatWindowFromGeometry({ geometry, scrollTop, viewportHeight, active = true, pinned = false, targetIndex, velocity }: {
  geometry: ChatRowGeometry;
  scrollTop: number;
  viewportHeight: number;
  active?: boolean;
  pinned?: boolean;
  targetIndex?: number;
  velocity?: number;
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
  const signedVelocity = targetIndex === undefined && !pinned ? effectiveScrollVelocity(velocity, height) : 0;
  const leadingOverscan = height + Math.ceil(Math.abs(signedVelocity) * 2);
  const overscanAbove = signedVelocity < 0 ? leadingOverscan : height;
  const overscanBelow = signedVelocity > 0 ? leadingOverscan : height;
  let start = Math.min(visibleStart, geometry.boundary(top - overscanAbove, true));
  let end = Math.max(visibleEnd, ceiling(top + height + overscanBelow));
  const limit = visibleEnd - visibleStart > CHAT_MOUNT_TARGET || signedVelocity !== 0 ? CHAT_MOUNT_MAX : CHAT_MOUNT_TARGET;
  if (end - start > limit) {
    const spare = Math.max(0, limit - (visibleEnd - visibleStart));
    const wantBefore = visibleStart - start;
    const wantAfter = end - visibleEnd;
    const before = signedVelocity < 0
      ? Math.min(wantBefore, spare - Math.min(wantAfter, Math.floor(spare / 4)))
      : signedVelocity > 0
        ? Math.min(wantBefore, Math.max(Math.min(wantBefore, Math.floor(spare / 4)), spare - wantAfter))
        : Math.min(wantBefore, Math.floor(spare / 2));
    start = Math.max(start, visibleStart - before);
    end = Math.min(count, Math.max(visibleEnd, start + limit));
  }
  return { start, end, top: geometry.offset(start), bottom: total - geometry.offset(end), total };
}

export function sameMountedChatWindow(left: MountedChatWindow, right: MountedChatWindow): boolean {
  return left.start === right.start && left.end === right.end && left.top === right.top
    && left.bottom === right.bottom && left.total === right.total;
}

export function mountedChatWindowCoversViewport(
  current: MountedChatWindow,
  geometry: ChatRowGeometry,
  scrollTop: number,
  viewportHeight: number,
  velocity = 0,
): boolean {
  const count = geometry.count;
  const total = geometry.offset(count);
  if (count === 0 || current.total !== total || current.end <= current.start) return false;
  const height = Math.max(1, viewportHeight);
  const top = Math.max(0, Math.min(scrollTop, Math.max(0, total - height)));
  const baseMargin = Math.floor(height * 0.5);
  const signedVelocity = effectiveScrollVelocity(velocity, height);
  const leadingMargin = Math.max(baseMargin, Math.ceil(Math.abs(signedVelocity) * 1.5));
  const topMargin = signedVelocity < 0 ? leadingMargin : baseMargin;
  const bottomMargin = signedVelocity > 0 ? leadingMargin : baseMargin;
  const coversTop = current.start === 0 || current.top <= Math.max(0, top - topMargin);
  const coversBottom = current.end >= count || total - current.bottom >= Math.min(total, top + height + bottomMargin);
  return coversTop && coversBottom;
}
