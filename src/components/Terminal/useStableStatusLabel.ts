import { useRef } from "react";
import { initialStableLabel, nextStableLabel, type StableLabelState } from "./stableStatusLabel";

/**
 * The status text to render, with flapping between recent kinds of text settled by count.
 * A new `phaseKey` (working, idle, starting…) is a real change, never flapping, so it starts afresh.
 */
export function useStableStatusLabel(label: string, phaseKey: string): string {
  const state = useRef<{ phaseKey: string; label: StableLabelState } | null>(null);
  state.current = state.current && state.current.phaseKey === phaseKey
    ? { phaseKey, label: nextStableLabel(state.current.label, label) }
    : { phaseKey, label: initialStableLabel(label) };
  return state.current.label.shown;
}
