export interface ChatScrollGesture {
  touching: boolean;
  scrolling: boolean;
  awaitingMovement: boolean;
  observedScroll: boolean;
  stationaryFrames: number;
}

export type ChatScrollGestureEvent = "touchstart" | "touchend" | "scroll" | "wheel" | "scrollend" | "frame" | "reset";

export function idleChatScrollGesture(): ChatScrollGesture {
  return { touching: false, scrolling: false, awaitingMovement: false, observedScroll: false, stationaryFrames: 0 };
}

export interface ChatScrollGestureInput {
  supportsScrollEnd: boolean;
  atBottom: boolean;
  positionChanged: boolean;
  towardBottom?: boolean;
  /** Whether the wheel's direction has room to move; a wheel that cannot move never produces the scrollend that ends it. */
  canMove?: boolean;
}

const defaultInput: ChatScrollGestureInput = { supportsScrollEnd: true, atBottom: false, positionChanged: false };

export function chatScrollGestureAfterEvent(state: ChatScrollGesture, event: ChatScrollGestureEvent,
  input: ChatScrollGestureInput = defaultInput): ChatScrollGesture {
  switch (event) {
    case "touchstart": return { touching: true, scrolling: false, awaitingMovement: false, observedScroll: false, stationaryFrames: 0 };
    case "touchend":
      if (!state.touching) return state;
      return { touching: false, scrolling: !input.supportsScrollEnd || state.scrolling || (!state.observedScroll && input.positionChanged),
        awaitingMovement: !input.supportsScrollEnd, observedScroll: state.observedScroll, stationaryFrames: 0 };
    case "frame": {
      if (state.touching || !state.scrolling) return state;
      const stationaryFrames = input.positionChanged ? 0 : state.stationaryFrames + 1;
      return { ...state, stationaryFrames, scrolling: stationaryFrames < 3, awaitingMovement: stationaryFrames < 3 && state.awaitingMovement };
    }
    case "wheel":
      if (input.canMove !== false) return { ...state, scrolling: true, stationaryFrames: 0 };
      return input.towardBottom ? { ...state, scrolling: false, awaitingMovement: false } : state;
    case "scroll":
      if (!input.supportsScrollEnd) return { ...state,
        scrolling: state.scrolling || ((state.touching || state.awaitingMovement) && input.positionChanged),
        observedScroll: true, stationaryFrames: input.positionChanged ? 0 : state.stationaryFrames };
      return state.touching ? { ...state, scrolling: true, observedScroll: true } : state;
    case "scrollend": return { ...state, scrolling: false, awaitingMovement: false, observedScroll: true };
    case "reset": return idleChatScrollGesture();
  }
}

export function isChatUserScrolling(state: ChatScrollGesture): boolean {
  return state.touching || state.scrolling;
}
