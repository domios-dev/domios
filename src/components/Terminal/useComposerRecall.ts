import { useRef } from "react";
import { recallPrompt, rememberPrompt, swapStash, type RecallPosition } from "./composerModels";

export function useComposerRecall(tabId: string) {
  const initial = () => ({ tabId, history: [] as string[], stashed: "", position: null as RecallPosition | null });
  const state = useRef(initial());
  if (state.current.tabId !== tabId) state.current = initial();
  return {
    remember(text: string) { state.current.history = rememberPrompt(state.current.history, text); state.current.position = null; },
    reset() { state.current.position = null; },
    move(draft: string, direction: -1 | 1) {
      if (!state.current.history.length) return undefined;
      const next = recallPrompt(state.current.history, state.current.position, draft, direction);
      state.current.position = next.position;
      return next.text;
    },
    stash(draft: string) {
      const next = swapStash(draft, state.current.stashed);
      state.current.stashed = next.stashed;
      state.current.position = null;
      return next.draft;
    },
  };
}
