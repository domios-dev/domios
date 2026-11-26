import { flushSync } from "react-dom";
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
  runProgrammaticScrollWrite,
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

import { chatLayoutScrollTarget, chatScrollWriteTarget, chatUserScrollChanged, chatViewportLayoutChanged, shouldCaptureChatAnchor, shouldRevealNewerChatRows } from "./chatScrollDecisions";
import { chatOverflowAnchorFor } from "./chatAnchorAuthority";
import { chatScrollDiagnostic } from "./chatScrollDiagnostic";
import { chatRevealIndex } from "./chatAnchorModel";
import { useChatScrollGesture } from "./useChatScrollGesture";
import { chatScrollBookmark, chatScrollRestoreTarget, initialChatScrollAnchor, type ChatScrollState } from "./chatScrollMemory";
import { isChatScrollReady } from "./chatScrollReadiness";
import { journalChatNewerRevealed, journalChatViewportMode, type ChatViewportCause } from "./chatViewportJournal";

const SHOW_SCROLL_DOWN_THRESHOLD_PX = 240;

interface PendingPrependSnapshot extends ChatPrependSnapshot {
  version: unknown;
  anchor: ChatContentAnchor | null;
  preserveTop: boolean;
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
  /** Rows the logical window holds back below a detached reader; `onNewerEdge` renders them, true when it did. */
  newerCount?: number;
  onNewerEdge?: () => boolean;
  journalTabId?: string;
  userScrollingRef?: MutableRefObject<boolean>;
  /** Releases layout shift held off-screen during the gesture; returns the scroll delta that keeps the view still. */
  foldLayoutDrift?: () => number;
  /** Synchronously prepares the mounted window at the start of loaded history for instant Home navigation. */
  onJumpToTop?: () => void;
  /** Synchronously prepares the logical and mounted window at the latest message for instant End navigation. */
  onJumpToLatest?: () => void;
}

export interface UseChatScrollResult {
  scrollRef: RefObject<HTMLDivElement | null>;
  showScrollDown: boolean;
  gestureOwned: boolean;
  scrollToBottom: () => void;
  followSend: () => void;
  prepareForHistoryPrepend: (mode?: "anchor" | "top") => void;
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
  newerCount = 0,
  onNewerEdge,
  journalTabId,
  userScrollingRef: sharedUserScrollingRef,
  foldLayoutDrift,
  onJumpToTop,
  onJumpToLatest,
}: UseChatScrollOptions): UseChatScrollResult {
  const ownScrollRef = useRef<HTMLDivElement | null>(null);
  const scrollRef = externalScrollRef ?? ownScrollRef;
  const sessionKeyRef = useRef(sessionKey);
  const restoredKeyRef = useRef("");
  const persisted = getPersistedChatScrollState(sessionKey);
  const initial = initialChatScrollAnchor(persisted);
  const modeRef = useRef<ChatViewportMode>(initial.pinned ? "pinned" : "detached");
  const prependSnapshotRef = useRef<PendingPrependSnapshot | null>(null);
  const prependVersionRef = useRef(prependVersion);
  prependVersionRef.current = prependVersion;
  const lastViewportRef = useRef<ChatViewportGeometry>({
    scrollTop: initial.top, scrollHeight: 0, clientHeight: 0,
  });
  const contentAnchorRef = useRef<ChatContentAnchor | null>(initial.anchor);
  const visibleSnapshotRef = useRef<{ key: string; state: ChatScrollState | null }>({ key: sessionKey, state: persisted });
  const hookScrollTopRef = useRef<number | null>(null);
  const observerRef = useRef<ResizeObserver | null>(null);
  const observedRef = useRef(new Set<Element>());
  const onModeChangeRef = useRef(onModeChange);
  onModeChangeRef.current = onModeChange;
  const newerRef = useRef({ count: newerCount, reveal: onNewerEdge, tabId: journalTabId });
  newerRef.current = { count: newerCount, reveal: onNewerEdge, tabId: journalTabId };
  const suppressNewerRevealRef = useRef(false);
  const detachCauseRef = useRef<ChatViewportCause>("restore");
  const [showScrollDown, setShowScrollDown] = useState(false);
  const gestureEndRef = useRef<() => void>(() => {});
  const gestureStartRef = useRef<() => void>(() => {});
  const gestureReachedBottomRef = useRef(false);
  const instantKeyRef = useRef<(target: "top" | "bottom") => boolean>(() => false);
  gestureStartRef.current = () => {
    hookScrollTopRef.current = null;
    gestureReachedBottomRef.current = false;
    if (scrollRef.current) scrollRef.current.style.overflowAnchor = chatOverflowAnchorFor(modeRef.current, true);
  };
  const gesture = useChatScrollGesture(scrollRef, active, sessionKey, gestureEndRef, gestureStartRef, sharedUserScrollingRef, instantKeyRef);
  const userScrollingRef = gesture.userScrolling;
  const gestureOwnedRef = useRef(gesture.owned);
  gestureOwnedRef.current = gesture.owned;
  const resetGesture = gesture.reset;
  const recordGestureScroll = gesture.recordScroll;
  const holdingView = gesture.holding;
  const heldView = gesture.held;

  if (sessionKeyRef.current !== sessionKey) {
    sessionKeyRef.current = sessionKey;
    const next = getPersistedChatScrollState(sessionKey);
    const initial = initialChatScrollAnchor(next);
    modeRef.current = initial.pinned ? "pinned" : "detached";
    lastViewportRef.current = { scrollTop: initial.top, scrollHeight: 0, clientHeight: 0 };
    prependSnapshotRef.current = null;
    contentAnchorRef.current = initial.anchor;
    visibleSnapshotRef.current = { key: sessionKey, state: next };
    hookScrollTopRef.current = null;
  }
  if (followTailRef) followTailRef.current = modeRef.current === "pinned";

  const persist = useCallback((el: HTMLDivElement) => {
    if (!active || loading || el.clientHeight === 0) return;
    const state: ChatScrollState = {
      mode: modeRef.current,
      scrollTop: el.scrollTop,
      anchor: modeRef.current === "detached" ? contentAnchorRef.current : null,
    };
    visibleSnapshotRef.current.state = state;
    setPersistedChatScrollState(sessionKeyRef.current, state);
  }, [active, loading]);

  useLayoutEffect(() => {
    if (!active) return;
    const snapshot = visibleSnapshotRef.current;
    return () => {
      if (snapshot.state) setPersistedChatScrollState(snapshot.key, snapshot.state);
    };
  }, [active, sessionKey]);

  // The scroll event a write produces must not read as the user moving the view.
  const writeScrollTop = useCallback((el: HTMLDivElement, value: number) => {
    if (userScrollingRef.current) return;
    const target = chatScrollWriteTarget(el, value, userScrollingRef.current);
    const before = el.scrollTop;
    if (target != null) runProgrammaticScrollWrite(el, () => { el.scrollTop = target; });
    lastViewportRef.current = { scrollTop: el.scrollTop, scrollHeight: el.scrollHeight, clientHeight: el.clientHeight };
    if (target != null) chatScrollDiagnostic(sessionKeyRef.current, "write", { before, after: el.scrollTop, owned: userScrollingRef.current });
    if (el.scrollTop !== before) {
      hookScrollTopRef.current = el.scrollTop;
    }
    onViewportChange?.();
  }, [onViewportChange, userScrollingRef]);

  const setMode = useCallback((mode: ChatViewportMode, el: HTMLDivElement, cause: ChatViewportCause) => {
    const changed = modeRef.current !== mode;
    modeRef.current = mode;
    if (el.style.overflowAnchor !== chatOverflowAnchorFor(mode, userScrollingRef.current)) el.style.overflowAnchor = chatOverflowAnchorFor(mode, userScrollingRef.current);
    contentAnchorRef.current = mode === "detached" ? captureChatContentAnchor(el) : null;
    if (followTailRef) followTailRef.current = mode === "pinned";
    if (changed) {
      chatScrollDiagnostic(sessionKeyRef.current, "mode", { mode, cause, scrollTop: el.scrollTop, scrollHeight: el.scrollHeight, clientHeight: el.clientHeight });
      if (mode === "detached") detachCauseRef.current = cause;
      journalChatViewportMode(newerRef.current.tabId, mode, cause);
      onModeChangeRef.current?.(mode);
    }
    persist(el);
  }, [followTailRef, persist]);

  const pinToBottom = useCallback(() => {
    const el = scrollRef.current;
    if (!el || !active || !isChatScrollReady("pinned", loading, messageCount) || userScrollingRef.current) return;
    writeScrollTop(el, chatLayoutScrollTarget("pinned", el, null));
    setMode("pinned", el, "pin");
    setShowScrollDown(false);
  }, [active, loading, messageCount, setMode, userScrollingRef, writeScrollTop]);

  const scrollToBottom = useCallback(() => {
    if (userScrollingRef.current) {
      chatScrollDiagnostic(sessionKeyRef.current, "jump_ignored_owned", { owned: true });
      return;
    }
    resetGesture();
    pinToBottom();
  }, [pinToBottom, resetGesture, userScrollingRef]);

  // A detached viewport on the bottom edge cannot produce a scroll event to reattach, so it offers the jump
  // and renders rows held back below it; a later scroll down then reaches the real bottom and re-pins.
  const updateScrollDown = useCallback((el: HTMLDivElement) => {
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    const detached = modeRef.current === "detached";
    setShowScrollDown(detached && (distance > SHOW_SCROLL_DOWN_THRESHOLD_PX || isChatViewportNearBottom(el)));
    const newer = newerRef.current;
    if (
      shouldRevealNewerChatRows({
        mode: modeRef.current,
        newerCount: newer.count,
        userScrolling: userScrollingRef.current,
        gestureOwned: gestureOwnedRef.current,
        atBottom: isChatViewportAtBottom(el),
      })
      && newer.reveal?.()
    ) {
      newer.count = 0; // until the next render reports what is still held back
      journalChatNewerRevealed(newer.tabId, detachCauseRef.current);
    }
  }, [userScrollingRef]);

  const holdContentAnchor = useCallback((el: HTMLDivElement) => {
    if (userScrollingRef.current) return;
    const anchor = contentAnchorRef.current;
    const unhandledUserScroll = shouldCaptureChatAnchor(lastViewportRef.current, el, userScrollingRef.current);
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
  }, [persist, updateScrollDown, userScrollingRef, writeScrollTop]);

  const followSend = useCallback(() => {
    resetGesture();
    pinToBottom();
  }, [pinToBottom, resetGesture]);

  const handleContentLayout = useCallback(() => {
    if (active && modeRef.current === "pinned") pinToBottom();
  }, [active, loading, pinToBottom]);

  const prepareForHistoryPrepend = useCallback((mode: "anchor" | "top" = "anchor") => {
    const el = scrollRef.current;
    if (!el) return;
    if (mode === "top" && modeRef.current !== "detached") setMode("detached", el, "scroll");
    if (modeRef.current !== "detached") return;
    prependSnapshotRef.current = {
      scrollHeight: el.scrollHeight,
      scrollTop: el.scrollTop,
      version: prependVersionRef.current,
      preserveTop: mode === "top",
      ...capturePrependAnchor(el),
    };
  }, [setMode]);

  const handleScroll = useCallback((event: React.UIEvent<HTMLDivElement>) => {
    const el = event.currentTarget;
    if (!active || loading || el.clientHeight === 0) return;
    const hookScrollTop = hookScrollTopRef.current;
    hookScrollTopRef.current = null;
    if (!(hookScrollTop != null && el.scrollTop === hookScrollTop && !userScrollingRef.current)) {
      recordGestureScroll(lastViewportRef.current, el);
    }
    const userScrolling = userScrollingRef.current;
    const fromHookWrite = !userScrolling && hookScrollTop != null
      && el.scrollTop === hookScrollTop;
    const mode = fromHookWrite
      ? modeRef.current
      : chatViewportModeAfterScroll(
        modeRef.current,
        lastViewportRef.current,
        el,
        userScrolling,
        !userScrolling && newerRef.current.count === 0,
      );
    const layoutChanged = chatViewportLayoutChanged(lastViewportRef.current, el);
    if (userScrolling && !fromHookWrite) {
      if (isChatViewportAtBottom(el) && (!layoutChanged || chatUserScrollChanged(lastViewportRef.current, el))) {
        gestureReachedBottomRef.current = true;
      } else if (el.scrollTop < lastViewportRef.current.scrollTop) {
        gestureReachedBottomRef.current = false;
      }
    }
    chatScrollDiagnostic(sessionKeyRef.current, "scroll", { fromHookWrite, userScrolling, layoutChanged, previous: lastViewportRef.current,
      modeBefore: modeRef.current, modeAfter: mode, scrollTop: el.scrollTop, scrollHeight: el.scrollHeight, clientHeight: el.clientHeight });
    if (mode === "detached" && layoutChanged && !userScrolling) holdContentAnchor(el);
    if (mode === "pinned" && !isChatViewportAtBottom(el)) writeScrollTop(el, el.scrollHeight);
    lastViewportRef.current = { scrollTop: el.scrollTop, scrollHeight: el.scrollHeight, clientHeight: el.clientHeight };
    if (fromHookWrite || (layoutChanged && !userScrolling)) persist(el);
    else if (!userScrolling || mode !== modeRef.current) setMode(mode, el, "scroll");
    updateScrollDown(el);
  }, [active, loading, holdContentAnchor, persist, recordGestureScroll, setMode, updateScrollDown, userScrollingRef, writeScrollTop]);

  const revealMessage = useCallback((messageId: string) => {
    const el = scrollRef.current;
    if (!el) return false;
    const rows = Array.from(el.querySelectorAll<HTMLElement>("[data-message-id]"));
    const index = chatRevealIndex(rows.map((row) => ({ id: row.dataset.messageId ?? "", sourceMessageId: row.dataset.sourceMessageId })), messageId);
    const row = rows[index];
    if (!row) return false;
    resetGesture();
    writeScrollTop(el, row.getBoundingClientRect().top - el.getBoundingClientRect().top + el.scrollTop);
    // Detached, so a pinned tail cannot pull the revealed row back out of view.
    setMode("detached", el, "reveal");
    updateScrollDown(el);
    return true;
  }, [resetGesture, setMode, updateScrollDown, writeScrollTop]);

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
      observer.observe(child, { box: "border-box" });
      observed.add(child);
    }
  }, []);

  // Geometry outside React commits reaches the writer only here: scroller box, content border box (insets) and rows.
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el || !active || typeof ResizeObserver === "undefined") return;
    const observed = observedRef.current;
    const observer = new ResizeObserver(() => {
      if (el.clientHeight === 0) return;
      // A finger holding a pinned view (longer than a tap) keeps it where it is: content arriving underneath detaches
      // the view and offers the jump, rather than snapping to the new bottom when the finger lifts. A tap still re-pins.
      if (modeRef.current === "pinned" && holdingView() && !isChatViewportAtBottom(el)) {
        setMode("detached", el, "hold");
        updateScrollDown(el);
      } else if (modeRef.current === "pinned") pinToBottom();
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
  }, [active, holdContentAnchor, holdingView, observeContent, pinToBottom, setMode, updateScrollDown]);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el) observeContent(el);
  }, [contentVersion, observeContent]);

  const repairViewport = useCallback(() => {
    const el = scrollRef.current;
    if (!active) { restoredKeyRef.current = ""; return; }
    if (!el || el.clientHeight === 0 || !isChatScrollReady(modeRef.current, loading, messageCount) || userScrollingRef.current) return;
    if (el.style.overflowAnchor !== chatOverflowAnchorFor(modeRef.current, userScrollingRef.current)) el.style.overflowAnchor = chatOverflowAnchorFor(modeRef.current, userScrollingRef.current);

    if (shouldRestoreChatViewport(active, loading, messageCount, sessionKey, restoredKeyRef.current)) {
      restoredKeyRef.current = sessionKey;
      const bookmark = chatScrollBookmark(getPersistedChatScrollState(sessionKey));
      const anchored = bookmark.mode === "detached" ? chatScrollTopForAnchor(el, bookmark.anchor) : null;
      const target = chatScrollRestoreTarget(bookmark, anchored, el.scrollHeight - el.clientHeight);
      if (target.mode === "detached") {
        writeScrollTop(el, target.top);
        setMode("detached", el, "restore");
        updateScrollDown(el);
      } else {
        pinToBottom();
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
      if (newerRef.current.count > 0) suppressNewerRevealRef.current = true;
      if (prependSnapshot.preserveTop) {
        writeScrollTop(el, 0);
        contentAnchorRef.current = captureChatContentAnchor(el);
        persist(el);
        return;
      }
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
      pinToBottom();
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
    pinToBottom,
    userScrollingRef,
    sessionKey,
    setMode,
    updateScrollDown,
    writeScrollTop,
  ]);

  const jumpToTop = useCallback(() => {
    const el = scrollRef.current;
    if (!el || !active || loading || el.clientHeight === 0) return false;
    if (el.scrollTop <= 0 && modeRef.current === "detached") return false;
    const doc = el.ownerDocument;
    const hadFocus = doc.activeElement != null && el.contains(doc.activeElement);
    resetGesture();
    contentAnchorRef.current = null;
    if (el.scrollTop !== 0) {
      runProgrammaticScrollWrite(el, () => { el.scrollTop = 0; });
      hookScrollTopRef.current = 0;
      lastViewportRef.current = { scrollTop: 0, scrollHeight: el.scrollHeight, clientHeight: el.clientHeight };
    }
    flushSync(() => {
      onJumpToTop?.();
      setMode("detached", el, "scroll");
      contentAnchorRef.current = null;
    });
    writeScrollTop(el, 0);
    contentAnchorRef.current = captureChatContentAnchor(el);
    persist(el);
    updateScrollDown(el);
    if (hadFocus && !el.contains(doc.activeElement)) el.focus({ preventScroll: true });
    return true;
  }, [active, loading, onJumpToTop, persist, resetGesture, scrollRef, setMode, updateScrollDown, writeScrollTop]);

  const jumpToLatest = useCallback(() => {
    const el = scrollRef.current;
    if (!el || !active || loading || el.clientHeight === 0) return false;
    if (modeRef.current === "pinned" && newerRef.current.count === 0 && isChatViewportAtBottom(el)) return false;
    const doc = el.ownerDocument;
    const hadFocus = doc.activeElement != null && el.contains(doc.activeElement);
    resetGesture();
    flushSync(() => {
      setMode("pinned", el, "pin");
      onJumpToLatest?.();
      setShowScrollDown(false);
    });
    pinToBottom();
    if (hadFocus && !el.contains(doc.activeElement)) el.focus({ preventScroll: true });
    return true;
  }, [active, loading, onJumpToLatest, pinToBottom, resetGesture, scrollRef, setMode]);

  instantKeyRef.current = (target) => (target === "top" ? jumpToTop() : jumpToLatest());

  gestureEndRef.current = () => {
    const el = scrollRef.current;
    const reachedBottom = gestureReachedBottomRef.current;
    gestureReachedBottomRef.current = false;
    if (el) {
      const scrollBy = foldLayoutDrift?.() ?? 0;
      if (scrollBy !== 0) writeScrollTop(el, el.scrollTop + scrollBy);
      el.style.overflowAnchor = chatOverflowAnchorFor(modeRef.current, false);
      const computed = chatViewportModeAfterScroll(
        modeRef.current,
        lastViewportRef.current,
        el,
        true,
        newerRef.current.count === 0,
      );
      const afterScroll = computed === "detached" && reachedBottom && newerRef.current.count === 0 && isChatViewportAtBottom(el)
        ? "pinned"
        : computed;
      // Released after a hold with the bottom moved away underneath: stay put and offer the jump instead of snapping.
      const mode = afterScroll === "pinned" && heldView() && !isChatViewportAtBottom(el) ? "detached" : afterScroll;
      if (mode !== modeRef.current || mode === "detached" || shouldCaptureChatAnchor(lastViewportRef.current, el, true)) {
        setMode(mode, el, mode === afterScroll ? "scroll" : "hold");
      }
      if (mode === "detached") updateScrollDown(el);
    }
    repairViewport();
  };
  useLayoutEffect(repairViewport, [repairViewport]);

  // Rows held back below the window change no geometry, so their arrival — and a settled scroll landing on the
  // bottom edge — is checked here once gesture ownership has released.
  useLayoutEffect(() => {
    if (suppressNewerRevealRef.current) {
      suppressNewerRevealRef.current = false;
      return;
    }
    const el = scrollRef.current;
    if (el && active && !gesture.owned && newerCount > 0 && el.clientHeight > 0) updateScrollDown(el);
  }, [active, gesture.owned, newerCount, updateScrollDown]);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el && el.clientHeight > 0 && messageCount > 0
      && chatViewportLayoutChanged(lastViewportRef.current, el)) handleContentLayout();
  });

  return {
    scrollRef,
    showScrollDown,
    gestureOwned: gesture.owned,
    scrollToBottom,
    followSend,
    prepareForHistoryPrepend,
    handleScroll,
    handleContentLayout,
    revealMessage,
  };
}
