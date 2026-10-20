import { CHAT_TAIL_TOLERANCE_PX, chatTailGap, chatUserScrollChanged, chatViewportLayoutChanged } from "./chatScrollDecisions";
import { editFrontendScroll } from "../../lib/frontendUpdate/appState";
import { ChatScrollMemory, type ChatScrollState } from "./chatScrollMemory";
import { apiHost, relayTarget } from "../../lib/webApi";
export type { ChatScrollState } from "./chatScrollMemory";

export interface ChatViewportGeometry {
  scrollHeight: number;
  scrollTop: number;
  clientHeight: number;
}

export type ChatViewportMode = "pinned" | "detached";

export function isChatViewportAtBottom(viewport: ChatViewportGeometry): boolean {
  return chatTailGap(viewport) < CHAT_TAIL_TOLERANCE_PX;
}

export function snapChatViewportToBottom(viewport: ChatViewportGeometry): void {
  viewport.scrollTop = viewport.scrollHeight;
}

export function nextChatViewportMode(viewport: ChatViewportGeometry): ChatViewportMode {
  return isChatViewportAtBottom(viewport) ? "pinned" : "detached";
}

// Fractional device-pixel scroll offsets can leave the visual bottom short of the exact bottom.
const VIEWPORT_REATTACH_TOLERANCE_PX = 2;

export function isChatViewportNearBottom(viewport: ChatViewportGeometry): boolean {
  return viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight < VIEWPORT_REATTACH_TOLERANCE_PX;
}

export function chatViewportModeAfterScroll(
  currentMode: ChatViewportMode,
  previous: ChatViewportGeometry,
  viewport: ChatViewportGeometry,
  userScrolling = false,
): ChatViewportMode {
  const layoutChanged = chatViewportLayoutChanged(previous, viewport);
  if (layoutChanged && (!userScrolling || !chatUserScrollChanged(previous, viewport))) return currentMode;
  if (currentMode === "pinned" && isChatViewportAtBottom(viewport)) return "pinned";
  if (viewport.scrollTop < previous.scrollTop) return "detached";
  if (isChatViewportAtBottom(viewport)) return "pinned";
  return currentMode;
}

export interface ChatPrependSnapshot {
  scrollHeight: number;
  scrollTop: number;
}

export function chatScrollTopAfterPrepend(
  before: ChatPrependSnapshot,
  nextScrollHeight: number,
): number {
  return before.scrollTop + Math.max(0, nextScrollHeight - before.scrollHeight);
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

let persistedChatScrollStates = new ChatScrollMemory();
let persistedScope: string | null = null;

function chatScrollMemory(): ChatScrollMemory {
  try {
    const key = `codeterm:chat-scroll:${apiHost()}:${relayTarget()?.id ?? "self"}`;
    if (persistedScope !== key) {
      persistedChatScrollStates = new ChatScrollMemory(32, { storage: sessionStorage, key });
      persistedScope = key;
    }
  } catch {}
  return persistedChatScrollStates;
}

export function getPersistedChatScrollState(key: string): ChatScrollState | null {
  return chatScrollMemory().read(key);
}

export function setPersistedChatScrollState(key: string, state: ChatScrollState): void {
  editFrontendScroll(key, "chat", JSON.stringify(state));
  chatScrollMemory().save(key, state);
}

export function clearPersistedChatScrollState(key: string): void {
  chatScrollMemory().clear(key);
}
