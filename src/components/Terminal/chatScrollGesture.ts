export interface ChatScrollGesture {
  touching: boolean;
  scrolling: boolean;
  awaitingMovement: boolean;
  observedScroll: boolean;
}

export type ChatScrollGestureEvent = "touchstart" | "touchend" | "scroll" | "wheel" | "scrollend" | "reset";

export function idleChatScrollGesture(): ChatScrollGesture {
  return { touching: false, scrolling: false, awaitingMovement: false, observedScroll: false };
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
    case "touchstart": return { touching: true, scrolling: false, awaitingMovement: false, observedScroll: false };
    case "touchend":
      if (!state.touching) return state;
      return { touching: false, scrolling: input.supportsScrollEnd && (state.scrolling || (!state.observedScroll && input.positionChanged)),
        awaitingMovement: !input.supportsScrollEnd, observedScroll: state.observedScroll };
    case "wheel":
      if (input.canMove !== false) return { ...state, scrolling: true };
      return input.towardBottom ? { ...state, scrolling: false, awaitingMovement: false } : state;
    case "scroll":
      if (!input.supportsScrollEnd) return { ...state,
        scrolling: !input.atBottom && (state.scrolling || ((state.touching || state.awaitingMovement) && input.positionChanged)),
        awaitingMovement: !input.atBottom && state.awaitingMovement, observedScroll: true };
      return state.touching ? { ...state, scrolling: true, observedScroll: true } : state;
    case "scrollend": return { ...state, scrolling: false, awaitingMovement: false, observedScroll: true };
    case "reset": return idleChatScrollGesture();
  }
}

export function isChatUserScrolling(state: ChatScrollGesture): boolean {
  return state.touching || state.scrolling;
}
