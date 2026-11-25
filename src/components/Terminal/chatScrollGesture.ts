export interface ChatScrollGesture {
  touching: boolean;
  pointerDown: boolean;
  scrollbarDrag: boolean;
  scrolling: boolean;
  awaitingMovement: boolean;
  observedScroll: boolean;
  stationaryFrames: number;
}

export type ChatScrollGestureEvent =
  | "touchstart"
  | "touchend"
  | "pointerdown"
  | "pointerup"
  | "pointercancel"
  | "keydown"
  | "scroll"
  | "wheel"
  | "scrollend"
  | "frame"
  | "reset";

export function idleChatScrollGesture(): ChatScrollGesture {
  return {
    touching: false,
    pointerDown: false,
    scrollbarDrag: false,
    scrolling: false,
    awaitingMovement: false,
    observedScroll: false,
    stationaryFrames: 0,
  };
}

export interface ChatScrollGestureInput {
  supportsScrollEnd: boolean;
  atBottom: boolean;
  positionChanged: boolean;
  towardBottom?: boolean;
  /** Whether the wheel or key's direction has room to move; an input that cannot move never produces the scrollend that ends it. */
  canMove?: boolean;
  onScrollbar?: boolean;
}

const defaultInput: ChatScrollGestureInput = { supportsScrollEnd: true, atBottom: false, positionChanged: false };

const OVERLAY_SCROLLBAR_EDGE_PX = 16;

export function chatScrollKeyDirection(code: string, shiftKey = false): -1 | 1 | 0 {
  switch (code) {
    case "PageUp":
    case "Home":
    case "ArrowUp":
      return -1;
    case "PageDown":
    case "End":
    case "ArrowDown":
      return 1;
    case "Space":
      return shiftKey ? -1 : 1;
    default:
      return 0;
  }
}

export function isPointerOnChatScrollbar(
  rect: { left: number; right: number; top: number; bottom: number },
  box: { clientLeft: number; clientTop: number; clientWidth: number; clientHeight: number; offsetWidth: number; offsetHeight: number },
  point: { clientX: number; clientY: number },
): boolean {
  const innerLeft = rect.left + box.clientLeft;
  const innerRight = innerLeft + box.clientWidth;
  const innerTop = rect.top + box.clientTop;
  const innerBottom = innerTop + box.clientHeight;
  const hasClassicGutter = box.offsetWidth - box.clientWidth > 0 || box.offsetHeight - box.clientHeight > 0;
  if (point.clientX >= innerRight || point.clientX < innerLeft || point.clientY >= innerBottom || point.clientY < innerTop) {
    return true;
  }
  return !hasClassicGutter && point.clientX >= rect.right - OVERLAY_SCROLLBAR_EDGE_PX;
}

function releasePointer(state: ChatScrollGesture, input: ChatScrollGestureInput): ChatScrollGesture {
  if (!state.pointerDown) return state;
  const moved = state.scrolling || state.observedScroll || input.positionChanged;
  if (!input.supportsScrollEnd) {
    return {
      ...state,
      pointerDown: false,
      scrollbarDrag: false,
      scrolling: moved,
      awaitingMovement: moved,
      stationaryFrames: 0,
    };
  }
  return {
    ...state,
    pointerDown: false,
    scrollbarDrag: false,
    scrolling: state.scrolling || (!state.observedScroll && input.positionChanged),
    awaitingMovement: false,
    stationaryFrames: 0,
  };
}

export function chatScrollGestureAfterEvent(state: ChatScrollGesture, event: ChatScrollGestureEvent,
  input: ChatScrollGestureInput = defaultInput): ChatScrollGesture {
  switch (event) {
    case "touchstart":
      return {
        touching: true,
        pointerDown: false,
        scrollbarDrag: false,
        scrolling: false,
        awaitingMovement: false,
        observedScroll: false,
        stationaryFrames: 0,
      };
    case "touchend":
      if (!state.touching) return state;
      return {
        ...state,
        touching: false,
        scrolling: !input.supportsScrollEnd || state.scrolling || (!state.observedScroll && input.positionChanged),
        awaitingMovement: !input.supportsScrollEnd,
        stationaryFrames: 0,
      };
    case "pointerdown":
      return {
        ...state,
        pointerDown: true,
        scrollbarDrag: Boolean(input.onScrollbar),
        awaitingMovement: false,
        observedScroll: false,
        stationaryFrames: 0,
      };
    case "pointerup":
      return releasePointer(state, input);
    case "pointercancel":
      // Native scrollbar capture on Chromium can fire pointercancel before scroll events; wait for scrollend/mouseup.
      return state.scrollbarDrag ? state : releasePointer(state, input);
    case "frame": {
      if (state.touching || state.pointerDown || !state.scrolling) return state;
      const stationaryFrames = input.positionChanged ? 0 : state.stationaryFrames + 1;
      return { ...state, stationaryFrames, scrolling: stationaryFrames < 3, awaitingMovement: stationaryFrames < 3 && state.awaitingMovement };
    }
    case "wheel":
    case "keydown":
      if (input.canMove !== false) return { ...state, scrolling: true, stationaryFrames: 0 };
      return input.towardBottom ? { ...state, scrolling: false, awaitingMovement: false } : state;
    case "scroll": {
      const held = state.touching || state.pointerDown;
      if (!input.supportsScrollEnd) return {
        ...state,
        scrolling: state.scrolling || ((held || state.awaitingMovement) && input.positionChanged),
        observedScroll: true,
        stationaryFrames: input.positionChanged ? 0 : state.stationaryFrames,
      };
      return held ? { ...state, scrolling: true, observedScroll: true } : state;
    }
    case "scrollend":
      if (state.scrollbarDrag && state.observedScroll) {
        return { ...state, pointerDown: false, scrollbarDrag: false, scrolling: false, awaitingMovement: false, observedScroll: true, stationaryFrames: 0 };
      }
      return { ...state, scrolling: false, awaitingMovement: false, observedScroll: true };
    case "reset": return idleChatScrollGesture();
  }
}

export function isChatUserScrolling(state: ChatScrollGesture): boolean {
  return state.touching || state.pointerDown || state.scrolling;
}
