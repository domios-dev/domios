import { useLayoutEffect, useReducer, type AnimationEvent, type ReactNode } from "react";
import { deferredPresence, type PresencePhase } from "./deferredPresenceModel";

export function useDeferredPresence(present: boolean): {
  phase: PresencePhase;
  onAnimationStart: (event: AnimationEvent<HTMLElement>) => void;
  onAnimationEnd: (event: AnimationEvent<HTMLElement>) => void;
} {
  const [state, dispatch] = useReducer(deferredPresence, { phase: present ? "waiting" : "hidden", requested: present });
  useLayoutEffect(() => { dispatch({ type: "request", present }); }, [present]);
  return {
    phase: state.phase,
    onAnimationStart: event => { if (event.target === event.currentTarget && event.animationName === "ct-presence-enter") dispatch({ type: "entry-start" }); },
    onAnimationEnd: event => { if (event.target === event.currentTarget) dispatch({ type: event.animationName === "ct-presence-exit" ? "exit-end" : "entry-end" }); },
  };
}

export function DeferredPresence({ present, children, className = "" }: { present: boolean; children: ReactNode; className?: string }) {
  const { phase, onAnimationStart, onAnimationEnd } = useDeferredPresence(present);
  if (phase === "hidden") return null;
  return <div className={`ct-deferred-presence ${className}`} data-phase={phase} aria-hidden={!present || undefined}
    style={{ pointerEvents: present ? undefined : "none" }}
    onAnimationStart={onAnimationStart} onAnimationEnd={onAnimationEnd}>
    {children}
  </div>;
}
