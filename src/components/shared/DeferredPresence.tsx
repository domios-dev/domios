import { useLayoutEffect, useReducer, type ReactNode } from "react";
import { deferredPresence } from "./deferredPresenceModel";

export function DeferredPresence({ present, children, className = "" }: { present: boolean; children: ReactNode; className?: string }) {
  const [state, dispatch] = useReducer(deferredPresence, { phase: present ? "waiting" : "hidden", requested: present });
  useLayoutEffect(() => { dispatch({ type: "request", present }); }, [present]);
  if (state.phase === "hidden") return null;
  return <div className={`ct-deferred-presence ${className}`} data-phase={state.phase} aria-hidden={!present || undefined}
    style={{ pointerEvents: present ? undefined : "none" }}
    onAnimationStart={event => { if (event.target === event.currentTarget && event.animationName === "ct-presence-enter") dispatch({ type: "entry-start" }); }}
    onAnimationEnd={event => { if (event.target === event.currentTarget) dispatch({ type: event.animationName === "ct-presence-exit" ? "exit-end" : "entry-end" }); }}>
    {children}
  </div>;
}
