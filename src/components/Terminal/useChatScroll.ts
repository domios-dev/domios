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

const SHOW_SCROLL_DOWN_THRESHOLD_PX = 240;

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
  const onModeChangeRef = useRef(onModeChange);
  onModeChangeRef.current = onModeChange;
  const [showScrollDown, setShowScrollDown] = useState(false);

  if (sessionKeyRef.current !== sessionKey) {
    sessionKeyRef.current = sessionKey;
    const next = getPersistedChatScrollState(sessionKey);
    modeRef.current = next?.mode ?? "pinned";
    lastScrollTopRef.current = next?.scrollTop ?? 0;
    prependSnapshotRef.current = null;
  }
  if (followTailRef) followTailRef.current = modeRef.current === "pinned";

  const persist = useCallback((el: HTMLDivElement) => {
    setPersistedChatScrollState(sessionKeyRef.current, {
      mode: modeRef.current,
      scrollTop: el.scrollTop,
    });
  }, []);

  const setMode = useCallback((mode: ChatViewportMode, el: HTMLDivElement) => {
    const changed = modeRef.current !== mode;
    modeRef.current = mode;
    el.style.overflowAnchor = mode === "detached" && prependSnapshotRef.current == null ? "auto" : "none";
    if (followTailRef) followTailRef.current = mode === "pinned";
    if (changed) onModeChangeRef.current?.(mode);
    persist(el);
  }, [followTailRef, persist]);

  const scrollToBottom = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
    lastScrollTopRef.current = el.scrollTop;
    setMode("pinned", el);
    setShowScrollDown(false);
  }, [setMode]);

  const followSend = useCallback(() => {
    if (modeRef.current === "pinned") scrollToBottom();
  }, [scrollToBottom]);

  const prepareForHistoryPrepend = useCallback(() => {
    const el = scrollRef.current;
    if (!el || modeRef.current !== "detached") return;
    el.style.overflowAnchor = "none";
    prependSnapshotRef.current = {
      scrollHeight: el.scrollHeight,
      scrollTop: el.scrollTop,
      version: prependVersionRef.current,
      ...capturePrependAnchor(el),
    };
  }, []);

  const handleScroll = useCallback((event: React.UIEvent<HTMLDivElement>) => {
    const el = event.currentTarget;
    const mode = chatViewportModeAfterScroll(modeRef.current, lastScrollTopRef.current, el);
    lastScrollTopRef.current = el.scrollTop;
    setMode(mode, el);
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    setShowScrollDown(mode === "detached" && distance > SHOW_SCROLL_DOWN_THRESHOLD_PX);
  }, [setMode]);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      if (modeRef.current === "pinned") scrollToBottom();
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [scrollToBottom]);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el || !active || loading || messageCount === 0) return;
    el.style.overflowAnchor = modeRef.current === "detached" ? "auto" : "none";

    if (shouldRestoreChatViewport(active, loading, messageCount, sessionKey, restoredKeyRef.current)) {
      restoredKeyRef.current = sessionKey;
      const restored = getPersistedChatScrollState(sessionKey);
      if (restored?.mode === "detached") {
        el.scrollTop = restored.scrollTop;
        lastScrollTopRef.current = el.scrollTop;
        setMode("detached", el);
      } else {
        scrollToBottom();
      }
      return;
    }

    const prependSnapshot = prependSnapshotRef.current;
    if (prependSnapshot && Object.is(prependSnapshot.version, prependVersion)) {
      prependSnapshotRef.current = {
        ...prependSnapshot,
        scrollHeight: el.scrollHeight,
        scrollTop: el.scrollTop,
      };
    }
    if (
      prependSnapshot
      && !Object.is(prependSnapshot.version, prependVersion)
      && modeRef.current === "detached"
    ) {
      prependSnapshotRef.current = null;
      el.scrollTop = resolvePrependAnchor(el, prependSnapshot)
        ?? chatScrollTopAfterPrepend(prependSnapshot, el.scrollHeight);
      el.style.overflowAnchor = "auto";
      lastScrollTopRef.current = el.scrollTop;
      persist(el);
      return;
    }

    if (modeRef.current === "pinned") scrollToBottom();
  }, [active, contentVersion, loading, messageCount, persist, prependVersion, scrollToBottom, sessionKey, setMode]);

  return {
    scrollRef,
    showScrollDown,
    scrollToBottom,
    followSend,
    prepareForHistoryPrepend,
    handleScroll,
  };
}
