const BOTTOM_TOLERANCE_PX = 0.5;

export interface ChatViewportGeometry {
  scrollHeight: number;
  scrollTop: number;
  clientHeight: number;
}

export type ChatViewportMode = "pinned" | "detached";

export function isChatViewportAtBottom(viewport: ChatViewportGeometry): boolean {
  return viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight <= BOTTOM_TOLERANCE_PX;
}

export function snapChatViewportToBottom(viewport: ChatViewportGeometry): void {
  viewport.scrollTop = viewport.scrollHeight;
}

export function nextChatViewportMode(viewport: ChatViewportGeometry): ChatViewportMode {
  return isChatViewportAtBottom(viewport) ? "pinned" : "detached";
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

export interface ChatScrollState {
  mode: ChatViewportMode;
  scrollTop: number;
}

const persistedChatScrollStates = new Map<string, ChatScrollState>();
const MAX_PERSISTED_CHAT_SCROLL_STATES = 32;

export function getPersistedChatScrollState(key: string): ChatScrollState | null {
  return persistedChatScrollStates.get(key) ?? null;
}

export function setPersistedChatScrollState(key: string, state: ChatScrollState): void {
  persistedChatScrollStates.delete(key);
  persistedChatScrollStates.set(key, state);
  while (persistedChatScrollStates.size > MAX_PERSISTED_CHAT_SCROLL_STATES) {
    const oldest = persistedChatScrollStates.keys().next().value;
    if (oldest == null) break;
    persistedChatScrollStates.delete(oldest);
  }
}

export function clearPersistedChatScrollState(key: string): void {
  persistedChatScrollStates.delete(key);
}
