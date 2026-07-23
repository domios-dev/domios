import { useCallback, useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import {
  captureChatScrollAnchor,
  chatActivationRestoreTarget,
  getPersistedChatScrollState,
  nextChatViewportMode,
  resolveChatScrollTopForAnchor,
  setPersistedChatScrollState,
  shouldFollowChatSend,
  shouldRestoreChatViewport,
  snapChatViewportToBottom,
  type ChatScrollAnchor,
  type ChatScrollAnchorCandidate,
  type ChatViewportMode,
} from "./chatViewport";

const SHOW_SCROLL_DOWN_THRESHOLD_PX = 240;

function cssEscape(value: string): string {
  if (typeof CSS !== "undefined" && typeof CSS.escape === "function") return CSS.escape(value);
  return value.replace(/["\\]/g, "\\$&");
}

function collectAnchorCandidates(container: HTMLElement): ChatScrollAnchorCandidate[] {
  const candidates: ChatScrollAnchorCandidate[] = [];
  container.querySelectorAll<HTMLElement>("[data-message-id]").forEach((node) => {
    const id = node.dataset.messageId;
    if (id) candidates.push({ id, offsetTop: node.offsetTop });
  });
  return candidates;
}

function findAnchorOffsetTop(container: HTMLElement, elementId: string): number | null {
  const el = container.querySelector<HTMLElement>(`[data-message-id="${cssEscape(elementId)}"]`);
  return el ? el.offsetTop : null;
}

export interface UseChatScrollOptions {
  /** Identifies the transcript this viewport is anchored to (pane + agent session). */
  sessionKey: string;
  active: boolean;
  loading: boolean;
  messageCount: number;
}

export interface UseChatScrollResult {
  scrollRef: RefObject<HTMLDivElement | null>;
  contentRef: RefObject<HTMLDivElement | null>;
  showScrollDown: boolean;
  scrollToBottom: () => void;
  followSend: () => void;
  handleWheel: (event: React.WheelEvent<HTMLDivElement>) => void;
  handlePointerDown: (event: React.PointerEvent<HTMLDivElement>) => void;
  handlePointerMove: (event: React.PointerEvent<HTMLDivElement>) => void;
  handlePointerUp: () => void;
  handlePointerCancel: () => void;
  handleKeyDown: (event: React.KeyboardEvent<HTMLDivElement>) => void;
  handleScroll: (event: React.UIEvent<HTMLDivElement>) => void;
}

/**
 * The single scroll authority for the chat viewport. Every scrollTop write for
 * this pane's chat lives here: no other module may assign `.scrollTop`.
 *
 * Detached mode stores an explicit anchor (a message element id + its pixel
 * offset from the viewport top) instead of a bare "detached" flag, so the
 * reader's visual position survives height changes, history prepends, and a
 * full remount (panes genuinely unmount on tab switch) by re-resolving the
 * anchor element's new position rather than re-deriving a scrollTop delta.
 */
export function useChatScroll({ sessionKey, active, loading, messageCount }: UseChatScrollOptions): UseChatScrollResult {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);

  const sessionKeyRef = useRef(sessionKey);
  const modeRef = useRef<ChatViewportMode>(getPersistedChatScrollState(sessionKey)?.mode ?? "pinned");
  const anchorRef = useRef<ChatScrollAnchor | null>(getPersistedChatScrollState(sessionKey)?.anchor ?? null);
  if (sessionKeyRef.current !== sessionKey) {
    sessionKeyRef.current = sessionKey;
    const persisted = getPersistedChatScrollState(sessionKey);
    modeRef.current = persisted?.mode ?? "pinned";
    anchorRef.current = persisted?.anchor ?? null;
  }

  const scrollIntentRef = useRef(false);
  const pointerScrollRef = useRef(false);
  const lastRestoredKeyRef = useRef("");
  const followRafRef = useRef(0);

  const [showScrollDown, setShowScrollDown] = useState(false);

  const persist = useCallback(() => {
    setPersistedChatScrollState(sessionKeyRef.current, { mode: modeRef.current, anchor: anchorRef.current });
  }, []);

  const scrollToBottom = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    modeRef.current = "pinned";
    anchorRef.current = null;
    snapChatViewportToBottom(el);
    persist();
    setShowScrollDown(false);
  }, [persist]);

  const followSend = useCallback(() => {
    const wasPinned = modeRef.current === "pinned";
    if (!wasPinned) return;
    if (followRafRef.current) cancelAnimationFrame(followRafRef.current);
    followRafRef.current = requestAnimationFrame(() => {
      followRafRef.current = 0;
      if (shouldFollowChatSend(wasPinned, modeRef.current === "pinned")) scrollToBottom();
    });
  }, [scrollToBottom]);

  const handleScroll = useCallback((event: React.UIEvent<HTMLDivElement>) => {
    const el = event.currentTarget;
    const dist = el.scrollHeight - (el.scrollTop + el.clientHeight);
    const nextMode = nextChatViewportMode(
      modeRef.current,
      el,
      scrollIntentRef.current || pointerScrollRef.current,
    );
    scrollIntentRef.current = false;
    modeRef.current = nextMode;
    if (nextMode === "detached") {
      const content = contentRef.current;
      anchorRef.current = content ? captureChatScrollAnchor(el, collectAnchorCandidates(content)) : null;
    } else {
      anchorRef.current = null;
    }
    persist();
    setShowScrollDown(nextMode === "detached" && dist > SHOW_SCROLL_DOWN_THRESHOLD_PX);
  }, [persist]);

  const handleWheel = useCallback((event: React.WheelEvent<HTMLDivElement>) => {
    if (event.deltaY >= 0) return;
    modeRef.current = "detached";
    scrollIntentRef.current = true;
  }, []);

  const handlePointerDown = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    pointerScrollRef.current = event.clientX >= bounds.right - 20;
  }, []);

  const handlePointerMove = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    if (event.pointerType === "touch" || event.buttons !== 0) pointerScrollRef.current = true;
  }, []);

  const handlePointerUp = useCallback(() => {
    pointerScrollRef.current = false;
  }, []);

  const handlePointerCancel = useCallback(() => {
    pointerScrollRef.current = false;
  }, []);

  const handleKeyDown = useCallback((event: React.KeyboardEvent<HTMLDivElement>) => {
    if (["ArrowUp", "PageUp", "Home"].includes(event.key)) {
      modeRef.current = "detached";
      scrollIntentRef.current = true;
    }
  }, []);

  // Must run synchronously inside the ResizeObserver callback: RO delivers
  // after layout but before paint, so the write lands in the same frame.
  const syncViewportToContent = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    if (modeRef.current === "pinned") {
      snapChatViewportToBottom(el);
      return;
    }
    const content = contentRef.current;
    const anchor = anchorRef.current;
    if (!anchor || !content) return;
    const resolved = resolveChatScrollTopForAnchor(anchor, findAnchorOffsetTop(content, anchor.elementId));
    if (resolved == null) return;
    el.scrollTop = resolved;
  }, []);

  // Keyed on `loading`: the transcript nodes mount only after the loading gate
  // opens, so a mount-time-only effect would observe nothing and never re-run.
  useEffect(() => {
    if (loading) return;
    const el = scrollRef.current;
    const content = contentRef.current;
    if (!el || !content) return;
    // Native scroll anchoring fights this authority on net-zero-height window
    // shifts (top row dropped + bottom row appended) and drags us off bottom.
    el.style.overflowAnchor = "none";
    const observer = new ResizeObserver(syncViewportToContent);
    observer.observe(content);
    observer.observe(el);
    return () => observer.disconnect();
  }, [syncViewportToContent, loading]);

  // Commits can restructure the transcript without changing its total height,
  // which fires no ResizeObserver; re-sync before paint on every commit.
  useLayoutEffect(() => {
    if (loading) return;
    syncViewportToContent();
  });

  const applyRestoreTarget = useCallback((target: ReturnType<typeof chatActivationRestoreTarget>) => {
    const el = scrollRef.current;
    const content = contentRef.current;
    if (!el) return;
    if (target.mode === "anchor" && content) {
      const resolved = resolveChatScrollTopForAnchor(target.anchor, findAnchorOffsetTop(content, target.anchor.elementId));
      if (resolved != null) {
        modeRef.current = "detached";
        anchorRef.current = target.anchor;
        el.scrollTop = resolved;
        persist();
        return;
      }
    }
    scrollToBottom();
  }, [persist, scrollToBottom]);

  useLayoutEffect(() => {
    if (!shouldRestoreChatViewport(active, loading, messageCount, sessionKey, lastRestoredKeyRef.current)) return;
    lastRestoredKeyRef.current = sessionKey;
    const target = chatActivationRestoreTarget(getPersistedChatScrollState(sessionKey));
    applyRestoreTarget(target);
    const raf = requestAnimationFrame(() => applyRestoreTarget(target));
    return () => cancelAnimationFrame(raf);
  }, [active, loading, messageCount, sessionKey, applyRestoreTarget]);

  useEffect(() => () => {
    if (followRafRef.current) cancelAnimationFrame(followRafRef.current);
  }, []);

  return {
    scrollRef,
    contentRef,
    showScrollDown,
    scrollToBottom,
    followSend,
    handleWheel,
    handlePointerDown,
    handlePointerMove,
    handlePointerUp,
    handlePointerCancel,
    handleKeyDown,
    handleScroll,
  };
}
