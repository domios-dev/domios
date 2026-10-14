import { useCallback, useLayoutEffect, useRef, type MutableRefObject, type RefObject } from "react";
import { chatInnerScrollConsumesWheel, chatUserScrollChanged } from "./chatScrollDecisions";
import { isChatViewportAtBottom, type ChatViewportGeometry } from "./chatViewport";
import { chatScrollGestureAfterEvent, idleChatScrollGesture, isChatUserScrolling, type ChatScrollGestureEvent } from "./chatScrollGesture";

export function useChatScrollGesture(scrollRef: RefObject<HTMLDivElement | null>, active: boolean, sessionKey: string,
  onEnd: MutableRefObject<() => void>, onStart: MutableRefObject<() => void>) {
  const gesture = useRef(idleChatScrollGesture());
  const userScrolling = useRef(false);
  const supportsScrollEnd = useRef(true);
  const reset = useCallback(() => {
    gesture.current = idleChatScrollGesture();
    userScrolling.current = false;
  }, []);

  const recordScroll = useCallback((before: ChatViewportGeometry, viewport: ChatViewportGeometry) => {
    gesture.current = chatScrollGestureAfterEvent(gesture.current, "scroll", {
      supportsScrollEnd: supportsScrollEnd.current, atBottom: isChatViewportAtBottom(viewport),
      positionChanged: chatUserScrollChanged(before, viewport),
    });
    userScrolling.current = isChatUserScrolling(gesture.current);
  }, []);

  useLayoutEffect(() => {
    reset();
    const el = scrollRef.current;
    if (!el || !active) return;
    supportsScrollEnd.current = "onscrollend" in el;
    let touchStartViewport: ChatViewportGeometry | null = null;
    const transition = (event: ChatScrollGestureEvent, positionChanged = false, towardBottom = false, canMove?: boolean) => {
      const wasScrolling = userScrolling.current;
      gesture.current = chatScrollGestureAfterEvent(gesture.current, event, { supportsScrollEnd: supportsScrollEnd.current,
        atBottom: isChatViewportAtBottom(el), positionChanged, towardBottom, canMove });
      userScrolling.current = isChatUserScrolling(gesture.current);
      if (!wasScrolling && userScrolling.current) onStart.current();
      if (wasScrolling && !userScrolling.current) onEnd.current();
    };
    const start = () => {
      touchStartViewport = { scrollTop: el.scrollTop, scrollHeight: el.scrollHeight, clientHeight: el.clientHeight };
      transition("touchstart");
    };
    const end = () => {
      transition("touchend", touchStartViewport != null && chatUserScrollChanged(touchStartViewport, el));
      touchStartViewport = null;
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
      transition("wheel", false, event.deltaY > 0, chatInnerScrollConsumesWheel(el, event.deltaY));
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
      reset();
    };
  }, [active, onEnd, onStart, reset, scrollRef, sessionKey]);

  return { userScrolling, reset, recordScroll };
}
