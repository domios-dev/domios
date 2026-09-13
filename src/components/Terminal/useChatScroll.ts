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
  setPersistedChatScrollState,
  shouldRestoreChatViewport,
  type ChatPrependSnapshot,
  type ChatViewportMode,
} from "./chatViewport";
import {
  captureChatContentAnchor,
  chatScrollTopForAnchor,
  type ChatContentAnchor,
} from "./chatScrollAnchor";

const SHOW_SCROLL_DOWN_THRESHOLD_PX = 240;
const SCROLL_WRITE_TOLERANCE_PX = 1;

interface PendingPrependSnapshot extends ChatPrependSnapshot {
  version: unknown;
  anchorId: string | null;
  anchorOffset: number;
}

function capturePrependAnchor(el: HTMLDivElement): Pick<PendingPrependSnapshot, "anchorId" | "anchorOffset"> {
  const rows = Array.from(el.querySelectorAll<HTMLElement>("[data-message-id]"));
  let anchor = rows[0] ?? null;
  for (const row of rows) {
    if (row.offsetTop > el.scrollTop) break;
    anchor = row;
  }
  return {
    anchorId: anchor?.dataset.messageId ?? null,
    anchorOffset: anchor ? anchor.offsetTop - el.scrollTop : 0,
  };
}

function resolvePrependAnchor(el: HTMLDivElement, snapshot: PendingPrependSnapshot): number | null {
  if (!snapshot.anchorId) return null;
  const anchor = Array.from(el.querySelectorAll<HTMLElement>("[data-message-id]"))
    .find((row) => row.dataset.messageId === snapshot.anchorId);
  return anchor ? anchor.offsetTop - snapshot.anchorOffset : null;
}

export interface UseChatScrollOptions {
  sessionKey: string;
  active: boolean;
  loading: boolean;
  messageCount: number;
  contentVersion: unknown;
  prependVersion: unknown;
  followTailRef?: MutableRefObject<boolean>;
  onModeChange?: (mode: ChatViewportMode) => void;
}

export interface UseChatScrollResult {
  scrollRef: RefObject<HTMLDivElement | null>;
  showScrollDown: boolean;
  scrollToBottom: () => void;
  followSend: () => void;
  prepareForHistoryPrepend: () => void;
  handleScroll: (event: React.UIEvent<HTMLDivElement>) => void;
  /** Scrolls to the row whose `data-source-message-id` equals this id exactly; false when no such row is rendered. */
  revealMessage: (messageId: string) => boolean;
}

export function useChatScroll({
  sessionKey,
  active,
  loading,
  messageCount,
  contentVersion,
  prependVersion,
  followTailRef,
  onModeChange,
}: UseChatScrollOptions): UseChatScrollResult {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const sessionKeyRef = useRef(sessionKey);
  const restoredKeyRef = useRef("");
  const persisted = getPersistedChatScrollState(sessionKey);
  const modeRef = useRef<ChatViewportMode>(persisted?.mode ?? "pinned");
  const prependSnapshotRef = useRef<PendingPrependSnapshot | null>(null);
  const prependVersionRef = useRef(prependVersion);
  prependVersionRef.current = prependVersion;
  const lastScrollTopRef = useRef(persisted?.scrollTop ?? 0);
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
    lastScrollTopRef.current = next?.scrollTop ?? 0;
    prependSnapshotRef.current = null;
    contentAnchorRef.current = null;
    hookScrollTopRef.current = null;
  }
  if (followTailRef) followTailRef.current = modeRef.current === "pinned";

  const persist = useCallback((el: HTMLDivElement) => {
    setPersistedChatScrollState(sessionKeyRef.current, {
      mode: modeRef.current,
      scrollTop: el.scrollTop,
    });
  }, []);

  // The scroll event a write produces must not read as the user moving the view.
  const writeScrollTop = useCallback((el: HTMLDivElement, value: number) => {
    const before = el.scrollTop;
    el.scrollTop = value;
    lastScrollTopRef.current = el.scrollTop;
    if (el.scrollTop !== before) hookScrollTopRef.current = el.scrollTop;
  }, []);

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
    writeScrollTop(el, el.scrollHeight);
    setMode("pinned", el);
    setShowScrollDown(false);
  }, [setMode, writeScrollTop]);

  const holdContentAnchor = useCallback((el: HTMLDivElement) => {
    const anchor = contentAnchorRef.current;
    const clampedToBottom = el.scrollTop >= el.scrollHeight - el.clientHeight - SCROLL_WRITE_TOLERANCE_PX;
    const unhandledUserScroll = !clampedToBottom
      && Math.abs(el.scrollTop - lastScrollTopRef.current) >= SCROLL_WRITE_TOLERANCE_PX;
    if (!anchor || unhandledUserScroll) {
      contentAnchorRef.current = captureChatContentAnchor(el);
      return;
    }
    const target = chatScrollTopForAnchor(el, anchor);
    if (target == null) {
      contentAnchorRef.current = captureChatContentAnchor(el);
      return;
    }
    writeScrollTop(el, target);
    persist(el);
  }, [persist, writeScrollTop]);

  const followSend = useCallback(() => {
    if (modeRef.current === "pinned") scrollToBottom();
  }, [scrollToBottom]);

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
      && Math.abs(el.scrollTop - hookScrollTop) < SCROLL_WRITE_TOLERANCE_PX;
    const mode = fromHookWrite
      ? modeRef.current
      : chatViewportModeAfterScroll(modeRef.current, lastScrollTopRef.current, el);
    lastScrollTopRef.current = el.scrollTop;
    if (fromHookWrite) persist(el);
    else setMode(mode, el);
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    setShowScrollDown(mode === "detached" && distance > SHOW_SCROLL_DOWN_THRESHOLD_PX);
  }, [persist, setMode]);

  const revealMessage = useCallback((messageId: string) => {
    const el = scrollRef.current;
    if (!el) return false;
    const row = Array.from(el.querySelectorAll<HTMLElement>("[data-source-message-id]"))
      .find((candidate) => candidate.dataset.sourceMessageId === messageId);
    if (!row) return false;
    writeScrollTop(el, row.offsetTop);
    // Detached, so a pinned tail cannot pull the revealed row back out of view.
    setMode("detached", el);
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    setShowScrollDown(distance > SHOW_SCROLL_DOWN_THRESHOLD_PX);
    return true;
  }, [setMode, writeScrollTop]);

  const observeContent = useCallback((el: HTMLDivElement) => {
    const observer = observerRef.current;
    if (!observer) return;
    const observed = observedRef.current;
    const children = new Set<Element>(Array.from(el.children));
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
    if (!el || typeof ResizeObserver === "undefined") return;
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
  }, [holdContentAnchor, observeContent, scrollToBottom]);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el) observeContent(el);
  }, [contentVersion, observeContent]);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el || !active || loading || messageCount === 0) return;
    el.style.overflowAnchor = "none";

    if (shouldRestoreChatViewport(active, loading, messageCount, sessionKey, restoredKeyRef.current)) {
      restoredKeyRef.current = sessionKey;
      const restored = getPersistedChatScrollState(sessionKey);
      if (restored?.mode === "detached") {
        writeScrollTop(el, restored.scrollTop);
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

  return {
    scrollRef,
    showScrollDown,
    scrollToBottom,
    followSend,
    prepareForHistoryPrepend,
    handleScroll,
    revealMessage,
  };
}
