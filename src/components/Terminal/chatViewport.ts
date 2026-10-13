import { chatUserScrollChanged, chatViewportLayoutChanged } from "./chatScrollDecisions";
import { editFrontendScroll } from "../../lib/frontendUpdate/appState";
import { ChatScrollMemory, type ChatScrollState } from "./chatScrollMemory";
export type { ChatScrollState } from "./chatScrollMemory";

const VIEWPORT_TOLERANCE_PX = 0.5;

export interface ChatViewportGeometry {
  scrollHeight: number;
  scrollTop: number;
  clientHeight: number;
}

export type ChatViewportMode = "pinned" | "detached";

export function isChatViewportAtBottom(viewport: ChatViewportGeometry): boolean {
  return viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight <= VIEWPORT_TOLERANCE_PX;
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
  if (viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight <= 0) return "pinned";
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

const persistedChatScrollStates = new ChatScrollMemory();

export function getPersistedChatScrollState(key: string): ChatScrollState | null {
  return persistedChatScrollStates.read(key);
}

export function setPersistedChatScrollState(key: string, state: ChatScrollState): void {
  editFrontendScroll(key, "chat", JSON.stringify(state));
  persistedChatScrollStates.save(key, state);
}

export function clearPersistedChatScrollState(key: string): void {
  persistedChatScrollStates.clear(key);
}
