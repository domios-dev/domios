import {
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
  type MutableRefObject,
  type RefObject,
} from "react";
import {
  chatScrollTopAfterPrepend,
  getPersistedChatScrollState,
  nextChatViewportMode,
  setPersistedChatScrollState,
  shouldRestoreChatViewport,
  type ChatPrependSnapshot,
  type ChatViewportMode,
} from "./chatViewport";

const SHOW_SCROLL_DOWN_THRESHOLD_PX = 240;

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
  const prependSnapshotRef = useRef<(ChatPrependSnapshot & { version: unknown }) | null>(null);
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
    prependSnapshotRef.current = {
      scrollHeight: el.scrollHeight,
      scrollTop: el.scrollTop,
      version: prependVersionRef.current,
    };
  }, []);

  const handleScroll = useCallback((event: React.UIEvent<HTMLDivElement>) => {
    const el = event.currentTarget;
    const movedUp = el.scrollTop < lastScrollTopRef.current;
    const mode = movedUp ? "detached" : nextChatViewportMode(el);
    lastScrollTopRef.current = el.scrollTop;
    setMode(mode, el);
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    setShowScrollDown(mode === "detached" && distance > SHOW_SCROLL_DOWN_THRESHOLD_PX);
  }, [setMode]);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el || !active || loading || messageCount === 0) return;
    el.style.overflowAnchor = "none";

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
        scrollHeight: el.scrollHeight,
        scrollTop: el.scrollTop,
        version: prependSnapshot.version,
      };
    }
    if (
      prependSnapshot
      && !Object.is(prependSnapshot.version, prependVersion)
      && modeRef.current === "detached"
    ) {
      prependSnapshotRef.current = null;
      el.scrollTop = chatScrollTopAfterPrepend(prependSnapshot, el.scrollHeight);
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
