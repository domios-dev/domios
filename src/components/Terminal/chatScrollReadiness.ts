import type { ChatViewportMode } from "./chatViewport";

export function isChatScrollReady(mode: ChatViewportMode, loading: boolean, messageCount: number): boolean {
  return messageCount > 0 && (!loading || mode === "pinned");
}
