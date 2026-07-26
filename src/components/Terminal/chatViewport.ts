const BOTTOM_TOLERANCE_PX = 2;

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

export function nextChatViewportMode(
  viewport: ChatViewportGeometry,
): ChatViewportMode {
  if (isChatViewportAtBottom(viewport)) return "pinned";
  return "detached";
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

export interface ChatScrollAnchorCandidate {
  id: string;
  offsetTop: number;
}

export interface ChatScrollAnchor {
  elementId: string;
  offsetFromViewportTop: number;
}

export interface ChatScrollState {
  mode: ChatViewportMode;
  anchor: ChatScrollAnchor | null;
}

/** Candidates must be in ascending DOM/offsetTop order (natural chat flow). */
export function findTopVisibleAnchorId(
  scrollTop: number,
  candidates: readonly ChatScrollAnchorCandidate[],
): string | null {
  let found: string | null = null;
  for (const candidate of candidates) {
    if (candidate.offsetTop <= scrollTop) found = candidate.id;
    else break;
  }
  return found ?? candidates[0]?.id ?? null;
}

export function captureChatScrollAnchor(
  viewport: ChatViewportGeometry,
  candidates: readonly ChatScrollAnchorCandidate[],
): ChatScrollAnchor | null {
  const elementId = findTopVisibleAnchorId(viewport.scrollTop, candidates);
  if (elementId == null) return null;
  const candidate = candidates.find((c) => c.id === elementId);
  if (!candidate) return null;
  return { elementId, offsetFromViewportTop: candidate.offsetTop - viewport.scrollTop };
}

export function resolveChatScrollTopForAnchor(
  anchor: ChatScrollAnchor,
  currentOffsetTop: number | null,
): number | null {
  if (currentOffsetTop == null) return null;
  return currentOffsetTop - anchor.offsetFromViewportTop;
}

export function chatActivationRestoreTarget(
  persisted: ChatScrollState | null,
): { mode: "bottom" } | { mode: "anchor"; anchor: ChatScrollAnchor } {
  if (persisted?.mode === "detached" && persisted.anchor) {
    return { mode: "anchor", anchor: persisted.anchor };
  }
  return { mode: "bottom" };
}

const persistedChatScrollStates = new Map<string, ChatScrollState>();

export function getPersistedChatScrollState(key: string): ChatScrollState | null {
  return persistedChatScrollStates.get(key) ?? null;
}

export function setPersistedChatScrollState(key: string, state: ChatScrollState): void {
  persistedChatScrollStates.set(key, state);
}

export function clearPersistedChatScrollState(key: string): void {
  persistedChatScrollStates.delete(key);
}
