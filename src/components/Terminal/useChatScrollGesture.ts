import { DEV_BUILD } from "../../lib/devBuild";
import { chatTraceEdge, startChatGestureTrace } from "./chatGestureTraceRuntime";
import { packGesture } from "./chatGestureTrace";
import { useCallback, useLayoutEffect, useRef, useState, type MutableRefObject, type RefObject } from "react";
import { chatInnerScrollConsumesWheel, chatUserScrollChanged } from "./chatScrollDecisions";
import { isChatViewportAtBottom, type ChatViewportGeometry } from "./chatViewport";
import {
  chatInstantScrollKey,
  chatScrollGestureAfterEvent,
  chatScrollKeyDirection,
  idleChatScrollGesture,
  isChatUserScrolling,
  isPointerOnChatScrollbar,
  type ChatScrollGestureEvent,
} from "./chatScrollGesture";
import { chatScrollDiagnostic } from "./chatScrollDiagnostic";

/** A touch shorter than this is a tap; a longer one holds the view. */
export const CHAT_TAP_MS = 300;

const TRACE_EVENT_ORDER: readonly ChatScrollGestureEvent[] = [
  "touchstart",
  "touchend",
  "scroll",
  "wheel",
  "scrollend",
  "frame",
  "reset",
  "pointerdown",
  "pointerup",
  "pointercancel",
  "keydown",
];

const EDITABLE_SELECTOR = "input, textarea, select, [contenteditable='true'], [contenteditable='']";
const SPACE_INTERACTIVE_SELECTOR = "button, a, summary, [role='button'], [role='checkbox']";
const INSTANT_SCROLL_WIDGET_SELECTOR = "[role='menu'], [role='listbox'], [role='tablist'], [role='slider'], [role='tree']";

function innerScrollConsumesDirection(el: HTMLDivElement, path: EventTarget[], direction: number): boolean {
  const view = el.ownerDocument.defaultView;
  if (!view) return false;
  for (const target of path) {
    if (target === el) break;
    if (!(target instanceof view.HTMLElement)) continue;
    const overflow = view.getComputedStyle(target).overflowY;
    if ((overflow === "auto" || overflow === "scroll") && chatInnerScrollConsumesWheel(target, direction)) return true;
  }
  return false;
}

export function useChatScrollGesture(scrollRef: RefObject<HTMLDivElement | null>, active: boolean, sessionKey: string,
  onEnd: MutableRefObject<() => void>, onStart: MutableRefObject<() => void>, sharedUserScrolling?: MutableRefObject<boolean>,
  onInstantKey?: MutableRefObject<(target: "top" | "bottom") => boolean>) {
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
    const doc = el.ownerDocument;
    const view = doc.defaultView;
    supportsScrollEnd.current = "onscrollend" in el;
    let stopTrace: (() => void) | undefined;
    let touchStartViewport: ChatViewportGeometry | null = null;
    let pointerStartViewport: ChatViewportGeometry | null = null;
    let frame: number | null = null;
    let frameTop = el.scrollTop;
    const watchMotion = () => {
      if (supportsScrollEnd.current || frame != null || gesture.current.touching || gesture.current.pointerDown || !userScrolling.current) return;
      frameTop = el.scrollTop;
      frame = window.requestAnimationFrame(observeMotion);
    };
    const observeMotion = () => {
      frame = null;
      if (gesture.current.touching || gesture.current.pointerDown || !userScrolling.current) return;
      transition("frame", el.scrollTop !== frameTop);
      watchMotion();
    };
    const transition = (
      event: ChatScrollGestureEvent,
      positionChanged = false,
      towardBottom = false,
      canMove?: boolean,
      onScrollbar?: boolean,
    ) => {
      const wasScrolling = userScrolling.current;
      gesture.current = chatScrollGestureAfterEvent(gesture.current, event, {
        supportsScrollEnd: supportsScrollEnd.current,
        atBottom: isChatViewportAtBottom(el),
        positionChanged,
        towardBottom,
        canMove,
        onScrollbar,
      });
      userScrolling.current = isChatUserScrolling(gesture.current);
      setOwned(userScrolling.current);
      if (DEV_BUILD) {
        chatTraceEdge(sessionKey, "gesture", [TRACE_EVENT_ORDER.indexOf(event), packGesture(gesture.current), gesture.current.stationaryFrames]);
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
    const pointerDown = (event: PointerEvent) => {
      if (event.pointerType === "touch" || event.button !== 0) return;
      lastTouchMs.current = 0;
      pointerStartViewport = { scrollTop: el.scrollTop, scrollHeight: el.scrollHeight, clientHeight: el.clientHeight };
      const onScrollbar = event.target === el && isPointerOnChatScrollbar(
        el.getBoundingClientRect(),
        {
          clientLeft: el.clientLeft,
          clientTop: el.clientTop,
          clientWidth: el.clientWidth,
          clientHeight: el.clientHeight,
          offsetWidth: el.offsetWidth,
          offsetHeight: el.offsetHeight,
        },
        { clientX: event.clientX, clientY: event.clientY },
      );
      transition("pointerdown", false, false, undefined, onScrollbar);
    };
    const releasePointerDown = () => {
      if (!gesture.current.pointerDown) return;
      const moved = pointerStartViewport != null && chatUserScrollChanged(pointerStartViewport, el);
      pointerStartViewport = null;
      transition("pointerup", moved);
      watchMotion();
    };
    const pointerCancel = () => {
      if (!gesture.current.pointerDown) return;
      const moved = pointerStartViewport != null && chatUserScrollChanged(pointerStartViewport, el);
      if (!gesture.current.scrollbarDrag) pointerStartViewport = null;
      transition("pointercancel", moved);
      watchMotion();
    };
    const pointerMove = (event: PointerEvent) => {
      if (gesture.current.pointerDown && event.buttons === 0) releasePointerDown();
    };
    const wheel = (event: WheelEvent) => {
      if (event.defaultPrevented || event.deltaY === 0 || event.ctrlKey) return;
      if (innerScrollConsumesDirection(el, event.composedPath(), event.deltaY)) return;
      lastTouchMs.current = 0;
      transition("wheel", false, event.deltaY > 0, chatInnerScrollConsumesWheel(el, event.deltaY));
      watchMotion();
    };
    const mouseDown = (event: MouseEvent) => {
      if (gesture.current.pointerDown || gesture.current.touching || event.button !== 0 || event.target !== el) return;
      const onScrollbar = isPointerOnChatScrollbar(
        el.getBoundingClientRect(),
        {
          clientLeft: el.clientLeft,
          clientTop: el.clientTop,
          clientWidth: el.clientWidth,
          clientHeight: el.clientHeight,
          offsetWidth: el.offsetWidth,
          offsetHeight: el.offsetHeight,
        },
        { clientX: event.clientX, clientY: event.clientY },
      );
      if (!onScrollbar) return;
      lastTouchMs.current = 0;
      pointerStartViewport = { scrollTop: el.scrollTop, scrollHeight: el.scrollHeight, clientHeight: el.clientHeight };
      transition("pointerdown", false, false, undefined, true);
    };
    const keyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.ctrlKey || event.metaKey || event.altKey) return;
      const target = event.target;
      if (!view || !(target instanceof view.Node) || !el.contains(target)) return;
      const direction = chatScrollKeyDirection(event.code, event.shiftKey);
      if (direction === 0) return;
      if (target instanceof view.HTMLElement) {
        if (target.isContentEditable || target.closest(EDITABLE_SELECTOR)) return;
        if (event.code === "Space" && target.closest(SPACE_INTERACTIVE_SELECTOR)) return;
      }
      if (innerScrollConsumesDirection(el, event.composedPath(), direction)) return;
      const instant = chatInstantScrollKey(event.code, event.shiftKey);
      if (instant != null && onInstantKey?.current && !gesture.current.pointerDown && !gesture.current.touching) {
        if (target instanceof view.HTMLElement && target.closest(INSTANT_SCROLL_WIDGET_SELECTOR)) return;
        lastTouchMs.current = 0;
        if (onInstantKey.current(instant)) event.preventDefault();
        return;
      }
      lastTouchMs.current = 0;
      transition("keydown", false, direction > 0, chatInnerScrollConsumesWheel(el, direction));
      watchMotion();
    };
    const scrollEnd = () => {
      if (gesture.current.scrollbarDrag && gesture.current.observedScroll) pointerStartViewport = null;
      transition("scrollend");
    };
    el.addEventListener("touchstart", start, { passive: true });
    doc.addEventListener("touchend", end, { passive: true });
    doc.addEventListener("touchcancel", end, { passive: true });
    el.addEventListener("pointerdown", pointerDown, { passive: true });
    el.addEventListener("mousedown", mouseDown, { passive: true });
    doc.addEventListener("pointerup", releasePointerDown, { passive: true });
    doc.addEventListener("mouseup", releasePointerDown, { passive: true });
    doc.addEventListener("pointercancel", pointerCancel, { passive: true });
    doc.addEventListener("pointermove", pointerMove, { passive: true });
    view?.addEventListener("blur", releasePointerDown);
    el.addEventListener("wheel", wheel, { passive: true });
    doc.addEventListener("keydown", keyDown);
    el.addEventListener("scrollend", scrollEnd, { passive: true });
    return () => {
      el.removeEventListener("touchstart", start);
      doc.removeEventListener("touchend", end);
      doc.removeEventListener("touchcancel", end);
      el.removeEventListener("pointerdown", pointerDown);
      el.removeEventListener("mousedown", mouseDown);
      doc.removeEventListener("pointerup", releasePointerDown);
      doc.removeEventListener("mouseup", releasePointerDown);
      doc.removeEventListener("pointercancel", pointerCancel);
      doc.removeEventListener("pointermove", pointerMove);
      view?.removeEventListener("blur", releasePointerDown);
      el.removeEventListener("wheel", wheel);
      doc.removeEventListener("keydown", keyDown);
      el.removeEventListener("scrollend", scrollEnd);
      if (frame != null) window.cancelAnimationFrame(frame);
      if (DEV_BUILD) stopTrace?.();
      reset();
    };
  }, [active, onEnd, onInstantKey, onStart, reset, scrollRef, sessionKey, userScrolling]);

  return { userScrolling, owned, reset, recordScroll, holding, held };
}
