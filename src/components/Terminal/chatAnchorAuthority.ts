import type { ChatViewportMode } from "./chatViewport";
import { chatOverflowAnchor } from "./chatScrollDecisions";

export function chatOverflowAnchorFor(mode: ChatViewportMode, owned: boolean): "none" | "auto" {
  return owned ? "none" : chatOverflowAnchor(mode);
}
