import {
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
  type MutableRefObject,
  type RefObject,
} from "react";
import {
  chatViewportModeAfterScroll,
  chatScrollTopAfterPrepend,
  getPersistedChatScrollState,
  isChatViewportAtBottom,
  isChatViewportNearBottom,
  setPersistedChatScrollState,
  shouldRestoreChatViewport,
  type ChatPrependSnapshot,
  type ChatViewportMode,
  type ChatViewportGeometry,
} from "./chatViewport";
import {
  captureChatContentAnchor,
  chatScrollTopForAnchor,
  type ChatContentAnchor,
} from "./chatScrollAnchor";

import { chatLayoutScrollTarget, chatViewportLayoutChanged, shouldCaptureChatAnchor } from "./chatScrollDecisions";
import { chatRevealIndex } from "./chatAnchorModel";

const SHOW_SCROLL_DOWN_THRESHOLD_PX = 240;

interface PendingPrependSnapshot extends ChatPrependSnapshot {
  version: unknown;
  anchor: ChatContentAnchor | null;
}

function capturePrependAnchor(el: HTMLDivElement): Pick<PendingPrependSnapshot, "anchor"> {
  return { anchor: captureChatContentAnchor(el) };
}

function resolvePrependAnchor(el: HTMLDivElement, snapshot: PendingPrependSnapshot): number | null {
  return snapshot.anchor ? chatScrollTopForAnchor(el, snapshot.anchor) : null;
}

export interface UseChatScrollOptions {
  scrollRef?: RefObject<HTMLDivElement | null>;
  sessionKey: string;
  active: boolean;
  loading: boolean;
  messageCount: number;
  contentVersion: unknown;
  prependVersion: unknown;
  followTailRef?: MutableRefObject<boolean>;
  onViewportChange?: () => void;
  onModeChange?: (mode: ChatViewportMode) => void;
}

export interface UseChatScrollResult {
  scrollRef: RefObject<HTMLDivElement | null>;
  showScrollDown: boolean;
  scrollToBottom: () => void;
  followSend: () => void;
  prepareForHistoryPrepend: () => void;
  handleScroll: (event: React.UIEvent<HTMLDivElement>) => void;
  handleContentLayout: () => void;
  /** Scrolls to the row whose `data-source-message-id` equals this id exactly; false when no such row is rendered. */
  revealMessage: (messageId: string) => boolean;
}

export function useChatScroll({
  scrollRef: externalScrollRef,
  sessionKey,
  active,
  loading,
  messageCount,
  contentVersion,
  prependVersion,
  followTailRef,
  onModeChange,
  onViewportChange,
}: UseChatScrollOptions): UseChatScrollResult {
  const ownScrollRef = useRef<HTMLDivElement | null>(null);
  const scrollRef = externalScrollRef ?? ownScrollRef;
  const sessionKeyRef = useRef(sessionKey);
  const restoredKeyRef = useRef("");
  const persisted = getPersistedChatScrollState(sessionKey);
  const modeRef = useRef<ChatViewportMode>(persisted?.mode ?? "pinned");
  const prependSnapshotRef = useRef<PendingPrependSnapshot | null>(null);
  const prependVersionRef = useRef(prependVersion);
  prependVersionRef.current = prependVersion;
  const lastViewportRef = useRef<ChatViewportGeometry>({
    scrollTop: persisted?.scrollTop ?? 0, scrollHeight: 0, clientHeight: 0,
  });
  const contentAnchorRef = useRef<ChatContentAnchor | null>(null);
  const hookScrollTopRef = useRef<number | null>(null);
  const observerRef = useRef<ResizeObserver | null>(null);
  const observedRef = useRef(new Set<Element>());
  const onModeChangeRef = useRef(onModeChange);
  onModeChangeRef.current = onModeChange;
  const [showScrollDown, setShowScrollDown] = useState(false);

  if (sessionKeyRef.current !== sessionKey) {
    sessionKeyRef.current = sessionKey;
    const next = getPersistedChatScrollState(sessionKey);
    modeRef.current = next?.mode ?? "pinned";
    lastViewportRef.current = { scrollTop: next?.scrollTop ?? 0, scrollHeight: 0, clientHeight: 0 };
    prependSnapshotRef.current = null;
    contentAnchorRef.current = null;
    hookScrollTopRef.current = null;
  }
  if (followTailRef) followTailRef.current = modeRef.current === "pinned";

  const persist = useCallback((el: HTMLDivElement) => {
    setPersistedChatScrollState(sessionKeyRef.current, {
      mode: modeRef.current,
      scrollTop: el.scrollTop,
      anchor: modeRef.current === "detached" ? contentAnchorRef.current : null,
    });
  }, []);

  // The scroll event a write produces must not read as the user moving the view.
  const writeScrollTop = useCallback((el: HTMLDivElement, value: number) => {
    const before = el.scrollTop;
    el.scrollTop = value;
    lastViewportRef.current = { scrollTop: el.scrollTop, scrollHeight: el.scrollHeight, clientHeight: el.clientHeight };
    if (el.scrollTop !== before) hookScrollTopRef.current = el.scrollTop;
    onViewportChange?.();
  }, [onViewportChange]);

  const setMode = useCallback((mode: ChatViewportMode, el: HTMLDivElement) => {
    const changed = modeRef.current !== mode;
    modeRef.current = mode;
    el.style.overflowAnchor = "none";
    contentAnchorRef.current = mode === "detached" ? captureChatContentAnchor(el) : null;
    if (followTailRef) followTailRef.current = mode === "pinned";
    if (changed) onModeChangeRef.current?.(mode);
    persist(el);
  }, [followTailRef, persist]);

  const scrollToBottom = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    writeScrollTop(el, chatLayoutScrollTarget("pinned", el, null));
    setMode("pinned", el);
    setShowScrollDown(false);
  }, [setMode, writeScrollTop]);

  // A detached viewport on the bottom edge cannot produce a scroll event to reattach, so it offers the jump.
  const updateScrollDown = useCallback((el: HTMLDivElement) => {
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    setShowScrollDown(modeRef.current === "detached"
      && (distance > SHOW_SCROLL_DOWN_THRESHOLD_PX || isChatViewportNearBottom(el)));
  }, []);

  const holdContentAnchor = useCallback((el: HTMLDivElement) => {
    const anchor = contentAnchorRef.current;
    const unhandledUserScroll = shouldCaptureChatAnchor(lastViewportRef.current, el);
    if (!anchor || unhandledUserScroll) {
      contentAnchorRef.current = captureChatContentAnchor(el);
      return;
    }
    const target = chatScrollTopForAnchor(el, anchor);
    if (target == null) {
      contentAnchorRef.current = captureChatContentAnchor(el);
      return;
    }
    writeScrollTop(el, chatLayoutScrollTarget("detached", el, target));
    persist(el);
    updateScrollDown(el);
  }, [persist, updateScrollDown, writeScrollTop]);

  const followSend = useCallback(() => {
    scrollToBottom();
  }, [scrollToBottom]);

  const handleContentLayout = useCallback(() => {
    if (active && !loading && modeRef.current === "pinned") scrollToBottom();
  }, [active, loading, scrollToBottom]);

  const prepareForHistoryPrepend = useCallback(() => {
    const el = scrollRef.current;
    if (!el || modeRef.current !== "detached") return;
    prependSnapshotRef.current = {
      scrollHeight: el.scrollHeight,
      scrollTop: el.scrollTop,
      version: prependVersionRef.current,
      ...capturePrependAnchor(el),
    };
  }, []);

  const handleScroll = useCallback((event: React.UIEvent<HTMLDivElement>) => {
    const el = event.currentTarget;
    const hookScrollTop = hookScrollTopRef.current;
    hookScrollTopRef.current = null;
    const fromHookWrite = hookScrollTop != null
      && el.scrollTop === hookScrollTop;
    const mode = fromHookWrite
      ? modeRef.current
      : chatViewportModeAfterScroll(modeRef.current, lastViewportRef.current, el);
    const layoutChanged = chatViewportLayoutChanged(lastViewportRef.current, el);
    if (mode === "detached" && layoutChanged) holdContentAnchor(el);
    if (mode === "pinned" && !isChatViewportAtBottom(el)) writeScrollTop(el, el.scrollHeight);
    lastViewportRef.current = { scrollTop: el.scrollTop, scrollHeight: el.scrollHeight, clientHeight: el.clientHeight };
    if (fromHookWrite || layoutChanged) persist(el);
    else setMode(mode, el);
    updateScrollDown(el);
  }, [holdContentAnchor, persist, setMode, updateScrollDown, writeScrollTop]);

  const revealMessage = useCallback((messageId: string) => {
    const el = scrollRef.current;
    if (!el) return false;
    const rows = Array.from(el.querySelectorAll<HTMLElement>("[data-message-id]"));
    const index = chatRevealIndex(rows.map((row) => ({ id: row.dataset.messageId ?? "", sourceMessageId: row.dataset.sourceMessageId })), messageId);
    const row = rows[index];
    if (!row) return false;
    writeScrollTop(el, row.offsetTop);
    // Detached, so a pinned tail cannot pull the revealed row back out of view.
    setMode("detached", el);
    updateScrollDown(el);
    return true;
  }, [setMode, updateScrollDown, writeScrollTop]);

  const observeContent = useCallback((el: HTMLDivElement) => {
    const observer = observerRef.current;
    if (!observer) return;
    const observed = observedRef.current;
    const children = new Set<Element>([...Array.from(el.children), ...Array.from(el.querySelectorAll("[data-chat-window-row]"))]);
    for (const child of observed) {
      if (children.has(child)) continue;
      observer.unobserve(child);
      observed.delete(child);
    }
    for (const child of children) {
      if (observed.has(child)) continue;
      observer.observe(child);
      observed.add(child);
    }
  }, []);

  // Content boxes are observed too: async image, iframe and diagram growth never re-renders the list.
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el || !active || typeof ResizeObserver === "undefined") return;
    const observed = observedRef.current;
    const observer = new ResizeObserver(() => {
      if (el.clientHeight === 0) return;
      if (modeRef.current === "pinned") scrollToBottom();
      else if (!prependSnapshotRef.current) holdContentAnchor(el);
    });
    observerRef.current = observer;
    observer.observe(el);
    observeContent(el);
    return () => {
      observer.disconnect();
      observed.clear();
      observerRef.current = null;
    };
  }, [active, holdContentAnchor, observeContent, scrollToBottom]);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el) observeContent(el);
  }, [contentVersion, observeContent]);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!active) { restoredKeyRef.current = ""; return; }
    if (!el || loading || messageCount === 0) return;
    el.style.overflowAnchor = "none";

    if (shouldRestoreChatViewport(active, loading, messageCount, sessionKey, restoredKeyRef.current)) {
      restoredKeyRef.current = sessionKey;
      const restored = getPersistedChatScrollState(sessionKey);
      if (restored?.mode === "detached") {
        const anchored = restored.anchor ? chatScrollTopForAnchor(el, restored.anchor) : null;
        writeScrollTop(el, anchored ?? restored.scrollTop);
        setMode("detached", el);
      } else {
        scrollToBottom();
      }
      return;
    }

    const prependSnapshot = prependSnapshotRef.current;
    if (
      prependSnapshot
      && !Object.is(prependSnapshot.version, prependVersion)
      && modeRef.current === "detached"
    ) {
      prependSnapshotRef.current = null;
      const anchor = contentAnchorRef.current;
      const anchoredTop = anchor ? chatScrollTopForAnchor(el, anchor) : null;
      writeScrollTop(
        el,
        anchoredTop
          ?? resolvePrependAnchor(el, prependSnapshot)
          ?? chatScrollTopAfterPrepend(prependSnapshot, el.scrollHeight),
      );
      if (anchoredTop == null) contentAnchorRef.current = captureChatContentAnchor(el);
      persist(el);
      return;
    }

    if (modeRef.current === "pinned") {
      scrollToBottom();
      return;
    }
    holdContentAnchor(el);
    if (prependSnapshot) {
      prependSnapshotRef.current = {
        ...prependSnapshot,
        scrollHeight: el.scrollHeight,
        scrollTop: el.scrollTop,
      };
    }
  }, [
    active,
    contentVersion,
    holdContentAnchor,
    loading,
    messageCount,
    persist,
    prependVersion,
    scrollToBottom,
    sessionKey,
    setMode,
    writeScrollTop,
  ]);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el && el.clientHeight > 0 && messageCount > 0
      && chatViewportLayoutChanged(lastViewportRef.current, el)) handleContentLayout();
  });

  return {
    scrollRef,
    showScrollDown,
    scrollToBottom,
    followSend,
    prepareForHistoryPrepend,
    handleScroll,
    handleContentLayout,
    revealMessage,
  };
}
