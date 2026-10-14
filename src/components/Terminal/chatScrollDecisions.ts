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

export function chatScrollWriteTarget(viewport: ChatViewportGeometry, requested: number, userScrolling: boolean): number | null {
  if (userScrolling || !Number.isFinite(requested)) return null;
  const target = Math.max(0, Math.min(requested, Math.max(0, viewport.scrollHeight - viewport.clientHeight)));
  return target === viewport.scrollTop ? null : target;
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
  return deltaY < 0 ? top > 0 : top < max;
}
