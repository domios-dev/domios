export type PresencePhase = "hidden" | "waiting" | "entering" | "visible" | "exiting";
export interface DeferredPresenceState { phase: PresencePhase; requested: boolean }
export type PresenceEvent = { type: "request"; present: boolean } | { type: "entry-start" | "entry-end" | "exit-end" };

export function deferredPresence(state: DeferredPresenceState, event: PresenceEvent): DeferredPresenceState {
  if (event.type === "request") {
    if (event.present) return { requested: true, phase: state.phase === "hidden" || state.phase === "exiting" ? "waiting" : state.phase };
    return { requested: false, phase: state.phase === "waiting" ? "hidden" : state.phase === "visible" ? "exiting" : state.phase };
  }
  if (event.type === "entry-start" && state.phase === "waiting") return { ...state, phase: "entering" };
  if (event.type === "entry-end" && (state.phase === "entering" || state.phase === "waiting")) return { ...state, phase: state.requested ? "visible" : "exiting" };
  if (event.type === "exit-end" && state.phase === "exiting") return { ...state, phase: "hidden" };
  return state;
}
