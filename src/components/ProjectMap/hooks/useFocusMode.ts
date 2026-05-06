import { useCallback, useEffect, useMemo, useReducer } from "react";
import type { FocusId } from "../types";

export const HISTORY_LIMIT = 64;

export interface FocusModeApi {
  focusId: FocusId;
  setFocus: (id: FocusId) => void;
  back: () => void;
  forward: () => void;
  canBack: boolean;
  canForward: boolean;
}

interface Options {
  enableShortcuts?: boolean;
}

export interface FocusState {
  history: FocusId[];
  cursor: number;
}

export type FocusAction =
  | { type: "set"; id: FocusId }
  | { type: "back" }
  | { type: "forward" };

export function focusReducer(state: FocusState, action: FocusAction): FocusState {
  switch (action.type) {
    case "set": {
      const trimmed = state.cursor < 0 ? [] : state.history.slice(0, state.cursor + 1);
      if (trimmed.length > 0 && trimmed[trimmed.length - 1] === action.id) {
        return state;
      }
      trimmed.push(action.id);
      while (trimmed.length > HISTORY_LIMIT) trimmed.shift();
      return { history: trimmed, cursor: trimmed.length - 1 };
    }
    case "back": {
      if (state.cursor <= 0) return state;
      return { ...state, cursor: state.cursor - 1 };
    }
    case "forward": {
      if (state.cursor >= state.history.length - 1) return state;
      return { ...state, cursor: state.cursor + 1 };
    }
  }
}

export function focusInitial(initial: FocusId): FocusState {
  return initial == null ? { history: [], cursor: -1 } : { history: [initial], cursor: 0 };
}

export function useFocusMode(initial: FocusId = null, opts: Options = {}): FocusModeApi {
  const [state, dispatch] = useReducer(focusReducer, initial, focusInitial);

  const focusId = useMemo(
    () => (state.cursor >= 0 && state.cursor < state.history.length ? state.history[state.cursor] : null),
    [state],
  );
  const canBack = state.cursor > 0;
  const canForward = state.cursor >= 0 && state.cursor < state.history.length - 1;

  const setFocus = useCallback((id: FocusId) => dispatch({ type: "set", id }), []);
  const back = useCallback(() => dispatch({ type: "back" }), []);
  const forward = useCallback(() => dispatch({ type: "forward" }), []);

  useEffect(() => {
    if (!opts.enableShortcuts) return;
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)) {
        return;
      }
      if (e.code === "KeyZ" && !e.metaKey && !e.ctrlKey && !e.altKey) {
        if (e.shiftKey) forward();
        else back();
        e.preventDefault();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [opts.enableShortcuts, back, forward]);

  return { focusId, setFocus, back, forward, canBack, canForward };
}
