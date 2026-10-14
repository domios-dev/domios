import type { ChatViewportGeometry, ChatViewportMode } from "./chatViewport";

export function chatViewportLayoutChanged(before: ChatViewportGeometry, after: ChatViewportGeometry): boolean {
  return before.scrollHeight !== after.scrollHeight || before.clientHeight !== after.clientHeight;
}

export function shouldCaptureChatAnchor(before: ChatViewportGeometry, after: ChatViewportGeometry, userScrolling = false): boolean {
  return (!chatViewportLayoutChanged(before, after) || (userScrolling && chatUserScrollChanged(before, after)))
    && Math.abs(after.scrollTop - before.scrollTop) >= 1
    && after.scrollTop < after.scrollHeight - after.clientHeight - 1;
}

export function chatLayoutScrollTarget(mode: ChatViewportMode, viewport: ChatViewportGeometry, anchorTop: number | null): number {
  return mode === "pinned" ? viewport.scrollHeight : anchorTop ?? viewport.scrollTop;
}

// Rounded scrollHeight and clientHeight put the browser's device-pixel bottom up to 1px either side of their difference.
export const CHAT_TAIL_TOLERANCE_PX = 1;

export function chatTailGap(viewport: ChatViewportGeometry): number {
  return viewport.scrollHeight - viewport.clientHeight - viewport.scrollTop;
}

/** A tail request overshoots so the browser clamps it to its exact bottom; it is skipped at or past the rounded bottom. */
export function chatScrollWriteTarget(viewport: ChatViewportGeometry, requested: number, userScrolling: boolean): number | null {
  if (userScrolling || !Number.isFinite(requested)) return null;
  const max = Math.max(0, viewport.scrollHeight - viewport.clientHeight);
  if (requested >= max) return chatTailGap(viewport) <= 0 ? null : viewport.scrollHeight;
  const target = Math.max(0, requested);
  return Math.abs(target - viewport.scrollTop) < CHAT_TAIL_TOLERANCE_PX ? null : target;
}

// A pinned tail has one JS writer; a detached reader lets the browser hold content through shifts above it, even mid-gesture.
export function chatOverflowAnchor(mode: ChatViewportMode): "none" | "auto" {
  return mode === "pinned" ? "none" : "auto";
}

export function chatUserScrollChanged(before: ChatViewportGeometry, after: ChatViewportGeometry): boolean {
  const max = Math.max(0, after.scrollHeight - after.clientHeight);
  const clampedTop = Math.max(0, Math.min(before.scrollTop, max));
  return after.scrollTop >= 0 && after.scrollTop <= max && Math.abs(after.scrollTop - clampedTop) > 0.5;
}

export function chatInnerScrollConsumesWheel(viewport: ChatViewportGeometry, deltaY: number): boolean {
  if (!Number.isFinite(deltaY) || deltaY === 0) return false;
  const max = Math.max(0, viewport.scrollHeight - viewport.clientHeight);
  const top = Math.max(0, Math.min(viewport.scrollTop, max));
  return deltaY < 0 ? top >= CHAT_TAIL_TOLERANCE_PX : max - top >= CHAT_TAIL_TOLERANCE_PX;
}
