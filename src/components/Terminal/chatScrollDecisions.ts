import type { ChatViewportGeometry, ChatViewportMode } from "./chatViewport";

export function chatViewportLayoutChanged(before: ChatViewportGeometry, after: ChatViewportGeometry): boolean {
  return before.scrollHeight !== after.scrollHeight || before.clientHeight !== after.clientHeight;
}

export function shouldCaptureChatAnchor(before: ChatViewportGeometry, after: ChatViewportGeometry): boolean {
  return !chatViewportLayoutChanged(before, after)
    && Math.abs(after.scrollTop - before.scrollTop) >= 1
    && after.scrollTop < after.scrollHeight - after.clientHeight - 1;
}

export function chatLayoutScrollTarget(mode: ChatViewportMode, viewport: ChatViewportGeometry, anchorTop: number | null): number {
  return mode === "pinned" ? viewport.scrollHeight : anchorTop ?? viewport.scrollTop;
}
