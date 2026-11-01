import { DEV_BUILD } from "../../lib/devBuild";
import { chatTraceEdge, startChatGestureTrace } from "./chatGestureTraceRuntime";
import { packGesture } from "./chatGestureTrace";
import { useCallback, useLayoutEffect, useRef, useState, type MutableRefObject, type RefObject } from "react";
import { chatInnerScrollConsumesWheel, chatUserScrollChanged } from "./chatScrollDecisions";
import { isChatViewportAtBottom, type ChatViewportGeometry } from "./chatViewport";
import { chatScrollGestureAfterEvent, idleChatScrollGesture, isChatUserScrolling, type ChatScrollGestureEvent } from "./chatScrollGesture";
import { chatScrollDiagnostic } from "./chatScrollDiagnostic";

/** A touch shorter than this is a tap; a longer one holds the view. */
export const CHAT_TAP_MS = 300;

export function useChatScrollGesture(scrollRef: RefObject<HTMLDivElement | null>, active: boolean, sessionKey: string,
  onEnd: MutableRefObject<() => void>, onStart: MutableRefObject<() => void>, sharedUserScrolling?: MutableRefObject<boolean>) {
  const gesture = useRef(idleChatScrollGesture());
  const [owned, setOwned] = useState(false);
  const ownUserScrolling = useRef(false);
  const userScrolling = sharedUserScrolling ?? ownUserScrolling;
  const supportsScrollEnd = useRef(true);
  // When the current touch began, and how long the last one lasted: a hold, unlike a tap, keeps the view where it is.
  const touchStartedAt = useRef<number | null>(null);
  const lastTouchMs = useRef(0);
  const holding = useCallback(() => touchStartedAt.current != null && performance.now() - touchStartedAt.current > CHAT_TAP_MS, []);
  const held = useCallback(() => lastTouchMs.current > CHAT_TAP_MS, []);
  const reset = useCallback(() => {
    gesture.current = idleChatScrollGesture();
    userScrolling.current = false;
    setOwned(false);
    if (DEV_BUILD) chatTraceEdge(sessionKey, "gesture", [6, packGesture(gesture.current), 0]);
  }, [userScrolling, sessionKey]);

  const recordScroll = useCallback((before: ChatViewportGeometry, viewport: ChatViewportGeometry) => {
    gesture.current = chatScrollGestureAfterEvent(gesture.current, "scroll", {
      supportsScrollEnd: supportsScrollEnd.current, atBottom: isChatViewportAtBottom(viewport),
      positionChanged: chatUserScrollChanged(before, viewport),
    });
    userScrolling.current = isChatUserScrolling(gesture.current);
    setOwned(userScrolling.current);
    if (DEV_BUILD) chatTraceEdge(sessionKey, "gesture", [2, packGesture(gesture.current), gesture.current.stationaryFrames]);
  }, [userScrolling, sessionKey]);

  useLayoutEffect(() => {
    reset();
    const el = scrollRef.current;
    if (!el || !active) return;
    supportsScrollEnd.current = "onscrollend" in el;
    let stopTrace: (() => void) | undefined;
    let touchStartViewport: ChatViewportGeometry | null = null;
    let frame: number | null = null;
    let frameTop = el.scrollTop;
    const watchMotion = () => {
      if (supportsScrollEnd.current || frame != null || gesture.current.touching || !userScrolling.current) return;
      frameTop = el.scrollTop;
      frame = window.requestAnimationFrame(observeMotion);
    };
    const observeMotion = () => {
      frame = null;
      if (gesture.current.touching || !userScrolling.current) return;
      transition("frame", el.scrollTop !== frameTop);
      watchMotion();
    };
    const transition = (event: ChatScrollGestureEvent, positionChanged = false, towardBottom = false, canMove?: boolean) => {
      const wasScrolling = userScrolling.current;
      gesture.current = chatScrollGestureAfterEvent(gesture.current, event, { supportsScrollEnd: supportsScrollEnd.current,
        atBottom: isChatViewportAtBottom(el), positionChanged, towardBottom, canMove });
      userScrolling.current = isChatUserScrolling(gesture.current);
      setOwned(userScrolling.current);
      if (DEV_BUILD) {
        chatTraceEdge(sessionKey, "gesture", [["touchstart", "touchend", "scroll", "wheel", "scrollend", "frame", "reset"].indexOf(event), packGesture(gesture.current), gesture.current.stationaryFrames]);
        if (event === "touchend" || event === "scrollend") chatTraceEdge(sessionKey, event);
      }
      if (event !== "frame" || (wasScrolling && !userScrolling.current)) chatScrollDiagnostic(sessionKey, "gesture", {
        event, supportsScrollEnd: supportsScrollEnd.current, owned: userScrolling.current, scrollTop: el.scrollTop,
      });
      if (!wasScrolling && userScrolling.current) onStart.current();
      if (wasScrolling && !userScrolling.current) onEnd.current();
    };
    const start = () => {
      if (DEV_BUILD) { stopTrace?.(); stopTrace = startChatGestureTrace(sessionKey, el, () => gesture.current); }
      touchStartViewport = { scrollTop: el.scrollTop, scrollHeight: el.scrollHeight, clientHeight: el.clientHeight };
      touchStartedAt.current = performance.now();
      transition("touchstart");
    };
    const end = () => {
      lastTouchMs.current = touchStartedAt.current == null ? 0 : performance.now() - touchStartedAt.current;
      touchStartedAt.current = null;
      transition("touchend", touchStartViewport != null && chatUserScrollChanged(touchStartViewport, el));
      touchStartViewport = null;
      watchMotion();
    };
    const wheel = (event: WheelEvent) => {
      if (event.defaultPrevented || event.deltaY === 0 || event.ctrlKey) return;
      const view = el.ownerDocument.defaultView;
      if (!view) return;
      for (const target of event.composedPath()) {
        if (target === el) break;
        if (!(target instanceof view.HTMLElement)) continue;
        const overflow = view.getComputedStyle(target).overflowY;
        if ((overflow === "auto" || overflow === "scroll") && chatInnerScrollConsumesWheel(target, event.deltaY)) return;
      }
      lastTouchMs.current = 0;
      transition("wheel", false, event.deltaY > 0, chatInnerScrollConsumesWheel(el, event.deltaY));
      watchMotion();
    };
    const scrollEnd = () => transition("scrollend");
    el.addEventListener("touchstart", start, { passive: true });
    el.ownerDocument.addEventListener("touchend", end, { passive: true });
    el.ownerDocument.addEventListener("touchcancel", end, { passive: true });
    el.addEventListener("wheel", wheel, { passive: true });
    el.addEventListener("scrollend", scrollEnd, { passive: true });
    return () => {
      el.removeEventListener("touchstart", start);
      el.ownerDocument.removeEventListener("touchend", end);
      el.ownerDocument.removeEventListener("touchcancel", end);
      el.removeEventListener("wheel", wheel);
      el.removeEventListener("scrollend", scrollEnd);
      if (frame != null) window.cancelAnimationFrame(frame);
      if (DEV_BUILD) stopTrace?.();
      reset();
    };
  }, [active, onEnd, onStart, reset, scrollRef, sessionKey, userScrolling]);

  return { userScrolling, owned, reset, recordScroll, holding, held };
}
