const BOTTOM_TOLERANCE_PX = 2;

export interface ChatViewportGeometry {
  scrollHeight: number;
  scrollTop: number;
  clientHeight: number;
}

export function isChatViewportAtBottom(viewport: ChatViewportGeometry): boolean {
  return viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight <= BOTTOM_TOLERANCE_PX;
}

export function snapChatViewportToBottom(viewport: ChatViewportGeometry): void {
  viewport.scrollTop = viewport.scrollHeight;
}

export function shouldFollowChatSend(wasPinned: boolean, isStillPinned: boolean): boolean {
  return wasPinned && isStillPinned;
}

export function shouldRestoreChatViewport(
  active: boolean,
  loading: boolean,
  messageCount: number,
  currentKey: string,
  restoredKey: string,
): boolean {
  return active && !loading && messageCount > 0 && currentKey !== restoredKey;
}
